use clap::Parser;
use solana_program_runtime::execution_budget::{
    SVMTransactionExecutionBudget, SVMTransactionExecutionCost,
};
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
use std::{cell::RefCell, fs::File, io::Read, path::Path, sync::Arc};

use crate::{
    build::{build_assembly, BuildConfig},
    debugger::Debugger,
    error::DebuggerError,
    parser::{parse_rodata, LineMap},
    repl::Repl,
};

mod adapter;
mod build;
mod debugger;
mod error;
mod parser;
mod repl;
mod syscalls;

/// Simple instruction meter for testing
#[derive(Debug, Clone, Default)]
pub struct DebugContextObject {
    /// Contains the register state at every instruction in order of execution
    trace_log: Vec<TraceLogEntry>,
    compute_budget: SVMTransactionExecutionBudget,
    execution_cost: SVMTransactionExecutionCost,
    compute_meter: RefCell<u64>,
}

impl ContextObject for DebugContextObject {
    fn trace(&mut self, state: [u64; 12]) {
        self.trace_log.push(state);
    }

    fn consume(&mut self, amount: u64) {
        let mut compute_meter = self.compute_meter.borrow_mut();
        *compute_meter = compute_meter.saturating_sub(amount);
    }

    fn get_remaining(&self) -> u64 {
        *self.compute_meter.borrow()
    }
}

impl DebugContextObject {
    /// Initialize with instruction meter
    pub fn new(
        compute_budget: SVMTransactionExecutionBudget,
        execution_cost: SVMTransactionExecutionCost,
    ) -> Self {
        Self {
            trace_log: Vec::new(),
            compute_budget,
            execution_cost,
            compute_meter: RefCell::new(compute_budget.compute_unit_limit),
        }
    }

    pub fn consume_checked(&self, amount: u64) -> Result<(), Box<dyn std::error::Error>> {
        let mut compute_meter = self.compute_meter.borrow_mut();
        let exceeded = *compute_meter < amount;
        *compute_meter = compute_meter.saturating_sub(amount);
        if exceeded {
            return Err(Box::new(DebuggerError::ComputationalBudgetExceeded));
        }
        Ok(())
    }

    pub fn get_execution_cost(&self) -> SVMTransactionExecutionCost {
        self.execution_cost
    }

    pub fn get_compute_budget(&self) -> SVMTransactionExecutionBudget {
        self.compute_budget
    }
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(
        short,
        long,
        value_name = "FILE",
        help = "Path to the assembly file (.s file)"
    )]
    file: String,

    #[arg(
        short,
        long,
        value_name = "LINKER",
        help = "Path to custom linker file (.ld file)"
    )]
    linker: Option<String>,

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

    // Build the assembly file
    let build_config = BuildConfig {
        assembly_file: args.file.clone(),
        linker_file: args.linker.clone(),
        debug: true, // Always build with debug information for debugging
    };

    let build_result = build_assembly(&build_config).unwrap_or_else(|e| {
        eprintln!("error:Failed to build assembly: {}", e);
        std::process::exit(1);
    });

    let mut loader = BuiltinProgram::new_loader(Config {
        enable_symbol_and_section_labels: true,
        ..Config::default()
    });

    // Logging syscalls
    loader
        .register_function("sol_log_", syscalls::SyscallLog::vm)
        .unwrap();
    loader
        .register_function("sol_log_64_", syscalls::SyscallLogU64::vm)
        .unwrap();
    let loader = Arc::new(loader);

    // Try to load DWARF line mapping from debug file or executable.
    let file_path = &build_result.shared_object_file;
    let debug_file_path = &build_result.object_file;
    let line_map = LineMap::from_elf_file(debug_file_path).ok();
    let rodata = parse_rodata(file_path, debug_file_path).ok();

    #[allow(unused_mut)]
    let mut executable = {
        let mut file =
            File::open(Path::new(&build_result.shared_object_file)).unwrap_or_else(|e| {
                eprintln!(
                    "error:Failed to open executable file '{}': {}",
                    build_result.shared_object_file, e
                );
                std::process::exit(1);
            });
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap_or_else(|e| {
            eprintln!(
                "error:Failed to read executable file '{}': {}",
                build_result.shared_object_file, e
            );
            std::process::exit(1);
        });
        Executable::<DebugContextObject>::from_elf(&elf, loader).map_err(|err| {
            eprintln!(
                "error:Failed to load executable '{}': {:?}",
                build_result.shared_object_file, err
            );
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

    let heap_size = args.heap.parse::<usize>().unwrap_or_else(|e| {
        eprintln!("error:Invalid heap size '{}': {}", args.heap, e);
        std::process::exit(1);
    });

    let mut context_object = DebugContextObject::new(
        SVMTransactionExecutionBudget::default(),
        SVMTransactionExecutionCost::default(),
    );
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
