use clap::Parser;
use solana_sbpf::{
    aligned_memory::AlignedMemory,
    ebpf,
    elf::Executable,
    error::ProgramResult,
    memory_region::{MemoryMapping, MemoryRegion},
    program::BuiltinProgram,
    static_analysis::TraceLogEntry,
    verifier::RequisiteVerifier,
    vm::{Config, ContextObject, EbpfVm},
};
use std::{fs::File, io::Read, path::Path, sync::Arc};

use crate::{
    debugger::Debugger,
    parser::{LineMap, parse_rodata},
    repl::Repl,
};

mod adapter;
mod debugger;
mod error;
mod parser;
mod repl;
mod syscalls;

/// Simple instruction meter for testing
#[derive(Debug, Clone, Default)]
pub struct DebugContextObject {
    /// Contains the register state at every instruction in order of execution
    pub trace_log: Vec<TraceLogEntry>,
    /// Maximal amount of instructions which still can be executed
    pub remaining: u64,
}

impl ContextObject for DebugContextObject {
    fn trace(&mut self, state: [u64; 12]) {
        self.trace_log.push(state);
    }

    fn consume(&mut self, amount: u64) {
        self.remaining = self.remaining.saturating_sub(amount);
    }

    fn get_remaining(&self) -> u64 {
        self.remaining
    }
}

impl DebugContextObject {
    /// Initialize with instruction meter
    pub fn new(remaining: u64) -> Self {
        Self {
            trace_log: Vec::new(),
            remaining,
        }
    }
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(
        short,
        long,
        value_name = "FILE",
        help = "Path to the sBPF executable (.so file)"
    )]
    file: String,

    #[arg(
        short,
        long,
        value_name = "DEBUG_FILE",
        help = "Path to the debug info file (.o file)"
    )]
    debug_file: Option<String>,

    #[arg(
        long,
        value_name = "BYTES",
        help = "Program input",
        default_value = "0"
    )]
    input: String,

    #[arg(long, value_name = "BYTES", help = "Heap memory", default_value = "0")]
    heap: String,

    #[arg(
        long,
        value_name = "COUNT",
        help = "Maximal number of instructions to execute",
        default_value = "10000"
    )]
    max_ixs: String,

    #[arg(long, help = "Run in adapter mode for VS Code extension")]
    adapter: bool,
}

fn main() {
    let args = Args::parse();

    let mut loader = BuiltinProgram::new_loader(Config {
        enable_symbol_and_section_labels: true,
        ..Config::default()
    });
    loader
        .register_function("sol_log_", syscalls::SyscallLog::vm)
        .unwrap();
    loader
        .register_function("sol_log_64_", syscalls::SyscallLogU64::vm)
        .unwrap();
    let loader = Arc::new(loader);

    // Try to load DWARF line mapping from debug file or executable.
    let file_path = args.file.as_ref();
    let debug_file_path = args.debug_file.as_ref().unwrap_or(&args.file);
    let line_map = LineMap::from_elf_file(debug_file_path).ok();
    let rodata = parse_rodata(file_path, debug_file_path).ok();

    #[allow(unused_mut)]
    let mut executable = {
        let mut file = File::open(Path::new(&args.file)).unwrap_or_else(|e| {
            eprintln!(
                "error:Failed to open executable file '{}': {}",
                args.file, e
            );
            std::process::exit(1);
        });
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap_or_else(|e| {
            eprintln!(
                "error:Failed to read executable file '{}': {}",
                args.file, e
            );
            std::process::exit(1);
        });
        Executable::<DebugContextObject>::from_elf(&elf, loader).map_err(|err| {
            eprintln!("error:Failed to load executable '{}': {:?}", args.file, err);
            format!("Executable constructor failed: {err:?}")
        })
    }
    .unwrap_or_else(|e| {
        eprintln!("error:{}", e);
        std::process::exit(1);
    });

    executable
        .verify::<RequisiteVerifier>()
        .unwrap_or_else(|e| {
            eprintln!("error:Failed to verify executable: {:?}", e);
            std::process::exit(1);
        });

    let mut mem: Vec<u8> = if !args.input.trim().is_empty() {
        let parts: Vec<&str> = args.input.split(',').collect();
        let mut bytes = Vec::with_capacity(parts.len());
        for part in parts {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            match trimmed.parse::<u8>() {
                Ok(b) => bytes.push(b),
                Err(_) => {
                    eprintln!("error:Invalid byte value in input: '{}'", trimmed);
                    std::process::exit(1);
                }
            }
        }
        bytes
    } else {
        Vec::new()
    };

    let max_instructions = args.max_ixs.parse::<u64>().unwrap_or_else(|e| {
        eprintln!(
            "error:Invalid max instructions value '{}': {}",
            args.max_ixs, e
        );
        std::process::exit(1);
    });

    let heap_size = args.heap.parse::<usize>().unwrap_or_else(|e| {
        eprintln!("error:Invalid heap size '{}': {}", args.heap, e);
        std::process::exit(1);
    });

    let mut context_object = DebugContextObject::new(max_instructions);
    let config = executable.get_config();
    let sbpf_version = executable.get_sbpf_version();
    let mut stack = AlignedMemory::<{ ebpf::HOST_ALIGN }>::zero_filled(config.stack_size());
    let stack_len = stack.len();
    let mut heap = AlignedMemory::<{ ebpf::HOST_ALIGN }>::zero_filled(heap_size);
    let regions: Vec<MemoryRegion> = vec![
        executable.get_ro_region(),
        MemoryRegion::new_writable_gapped(
            stack.as_slice_mut(),
            ebpf::MM_STACK_START,
            if !sbpf_version.dynamic_stack_frames() && config.enable_stack_frame_gaps {
                config.stack_frame_size as u64
            } else {
                0
            },
        ),
        MemoryRegion::new_writable(heap.as_slice_mut(), ebpf::MM_HEAP_START),
        MemoryRegion::new_writable(&mut mem, ebpf::MM_INPUT_START),
    ];

    let memory_mapping = MemoryMapping::new(regions, config, sbpf_version).unwrap_or_else(|e| {
        eprintln!("error:Failed to create memory mapping: {:?}", e);
        std::process::exit(1);
    });

    let mut vm = EbpfVm::new(
        executable.get_loader().clone(),
        executable.get_sbpf_version(),
        &mut context_object,
        memory_mapping,
        stack_len,
    );
    vm.registers[1] = ebpf::MM_INPUT_START;
    vm.registers[11] = executable.get_entrypoint_instruction_offset() as u64;
    // let config = executable.get_config();
    let initial_insn_count = vm.context_object_pointer.get_remaining();
    vm.previous_instruction_meter = initial_insn_count;
    vm.due_insn_count = 0;
    vm.program_result = ProgramResult::Ok(0);

    let mut debugger = Debugger::new(&mut vm, &executable);

    // Set the DWARF line mapping if available.
    if let Some(dwarf_map) = line_map {
        debugger.set_dwarf_line_map(dwarf_map);
    }

    if let Some(rodata) = rodata {
        debugger.set_rodata(rodata);
    }

    if args.adapter {
        // Run in adapter mode for VS Code extension.
        crate::adapter::run_adapter_loop(&mut debugger);
    } else {
        // Run in REPL mode.
        let mut repl = Repl::new(debugger);
        repl.start();
    }
}
