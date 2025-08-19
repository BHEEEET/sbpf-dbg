#![allow(dead_code)]

use std::collections::HashSet;

use serde_json::{Value, json};
use solana_sbpf::ebpf;
use solana_sbpf::elf::Executable;
use solana_sbpf::error::ProgramResult;
use solana_sbpf::interpreter::Interpreter;
use solana_sbpf::vm::{ContextObject, EbpfVm};

use crate::adapter::DebuggerInterface;
use crate::error::DebuggerResult;
use crate::parser::{LineMap, ROData};

#[derive(Debug)]
pub enum DebugMode {
    Step,
    Continue,
}

#[derive(Debug)]
pub enum DebugEvent {
    Breakpoint(u64, Option<usize>), // PC and optional line number
    Step(u64, Option<usize>),       // PC and optional line number
    Exit(u64),
    Error(String),
}

pub struct Debugger<'a, 'b, C: ContextObject> {
    pub(crate) interpreter: Interpreter<'a, 'b, C>,
    pub breakpoints: HashSet<u64>,        // PC-based breakpoints
    pub line_breakpoints: HashSet<usize>, // Line-based breakpoints
    pub dwarf_line_map: Option<LineMap>,  // DWARF line mapping
    pub rodata: Option<Vec<ROData>>,
    pub last_breakpoint: Option<u64>,
    pub debug_mode: DebugMode,
    pub stopped: bool,
    pub exit_code: u64,
    pub at_breakpoint: bool, // Whether we're currently stopped at a breakpoint
    pub last_breakpoint_pc: Option<u64>, // Last PC where we hit a breakpoint to avoid duplicates
    pub initial_compute_budget: u64, // Store the initial compute budget for tracking
}

impl<'a, 'b, C: ContextObject> Debugger<'a, 'b, C> {
    pub fn new(vm: &'a mut EbpfVm<'b, C>, executable: &'a Executable<C>) -> Self {
        let initial_compute_budget = vm.context_object_pointer.get_remaining();
        let interpreter = Interpreter::new(vm, executable, vm.registers);

        Self {
            interpreter: interpreter,
            breakpoints: HashSet::new(),
            line_breakpoints: HashSet::new(),
            dwarf_line_map: None,
            rodata: None,
            last_breakpoint: None,
            debug_mode: DebugMode::Continue,
            stopped: false,
            exit_code: 0,
            at_breakpoint: false,
            last_breakpoint_pc: None,
            initial_compute_budget,
        }
    }

    /// Set the DWARF line mapping after construction
    pub fn set_dwarf_line_map(&mut self, dwarf_map: LineMap) {
        self.dwarf_line_map = Some(dwarf_map);
    }

    pub fn set_rodata(&mut self, rodata: Vec<ROData>) {
        self.rodata = Some(rodata);
    }

    pub fn set_breakpoint(&mut self, pc: u64) {
        self.breakpoints.insert(pc);
    }

    pub fn set_breakpoint_at_line(&mut self, line: usize) -> Result<(), String> {
        if let Some(dwarf_map) = &self.dwarf_line_map {
            let pcs = dwarf_map.get_pcs_for_line(line);
            if !pcs.is_empty() {
                self.line_breakpoints.insert(line);
                for &pc in &pcs {
                    self.breakpoints.insert(pc);
                }
            }
        }
        Ok(())
    }

    pub fn remove_breakpoint_at_line(&mut self, line: usize) -> Result<(), String> {
        if let Some(dwarf_map) = &self.dwarf_line_map {
            let pcs = dwarf_map.get_pcs_for_line(line);
            if !pcs.is_empty() {
                self.line_breakpoints.remove(&line);
                for &pc in &pcs {
                    self.breakpoints.remove(&pc);
                }
            }
        }
        Ok(())
    }

    pub fn get_current_line(&self) -> Option<usize> {
        let pc = self.get_pc();
        self.get_line_for_pc(pc)
    }

    pub fn get_line_for_pc(&self, pc: u64) -> Option<usize> {
        if let Some(dwarf_map) = &self.dwarf_line_map {
            dwarf_map.get_line_for_pc(pc)
        } else {
            None
        }
    }

    pub fn get_pcs_for_line(&self, line: usize) -> Vec<u64> {
        if let Some(dwarf_map) = &self.dwarf_line_map {
            dwarf_map.get_pcs_for_line(line)
        } else {
            Vec::new()
        }
    }

    pub fn get_breakpoints_info(&self) -> String {
        let mut info = String::new();

        // PC-based breakpoints.
        if !self.breakpoints.is_empty() {
            info.push_str("PC breakpoints:\n");
            for &pc in &self.breakpoints {
                if let Some(line) = self.get_line_for_pc(pc) {
                    info.push_str(&format!("  PC 0x{:x} (line {})\n", pc, line));
                } else {
                    info.push_str(&format!("  PC 0x{:x}\n", pc));
                }
            }
        }

        // Line-based breakpoints.
        if !self.line_breakpoints.is_empty() {
            info.push_str("Line breakpoints:\n");
            for &line in &self.line_breakpoints {
                let pcs = self.get_pcs_for_line(line);
                if !pcs.is_empty() {
                    info.push_str(&format!("  Line {} (PCs: ", line));
                    for (i, &pc) in pcs.iter().enumerate() {
                        if i > 0 {
                            info.push_str(", ");
                        }
                        info.push_str(&format!("0x{:x}", pc));
                    }
                    info.push_str(")\n");
                }
            }
        }

        if info.is_empty() {
            info.push_str("No breakpoints set\n");
        }

        info
    }

    pub fn set_debug_mode(&mut self, debug_mode: DebugMode) {
        self.debug_mode = debug_mode;
    }

    /// Consume the accumulated due_insn_count from the VM
    fn consume_instruction_cost(&mut self) {
        let due_insn_count = self.interpreter.vm.due_insn_count;
        if due_insn_count > 0 {
            self.interpreter
                .vm
                .context_object_pointer
                .consume(due_insn_count);
            self.interpreter.vm.due_insn_count = 0;
        }
    }

    /// Run the debugger.
    pub fn run(&mut self) -> DebuggerResult<DebugEvent> {
        match self.debug_mode {
            DebugMode::Step => {
                let current_pc = self.get_pc();

                // If we're at a breakpoint, execute the instruction and then check for next breakpoint
                if self.at_breakpoint {
                    if self.interpreter.step() {
                        // Consume instruction cost after successful step
                        self.consume_instruction_cost();

                        self.at_breakpoint = false;
                        self.last_breakpoint_pc = None; // Clear the last breakpoint PC
                        // After executing, check if the new PC has a breakpoint
                        let new_pc = self.get_pc();
                        if self.breakpoints.contains(&new_pc) {
                            self.at_breakpoint = true;
                            self.last_breakpoint_pc = Some(new_pc);
                            let line_number = self.get_line_for_pc(new_pc);
                            return Ok(DebugEvent::Breakpoint(new_pc, line_number));
                        } else {
                            // No breakpoint at new PC, return Step event
                            let line_number = self.get_line_for_pc(new_pc);
                            return Ok(DebugEvent::Step(new_pc, line_number));
                        }
                    } else if let ProgramResult::Ok(result) = self.interpreter.vm.program_result {
                        self.consume_instruction_cost();
                        return Ok(DebugEvent::Exit(result));
                    } else if let ProgramResult::Err(err) = &self.interpreter.vm.program_result {
                        let error_msg =
                            format!("Program error at PC 0x{:016x}: {:?}", current_pc, err);
                        return Ok(DebugEvent::Error(error_msg));
                    } else {
                        let error_msg =
                            format!("Unknown program error at PC 0x{:016x}", current_pc);
                        return Ok(DebugEvent::Error(error_msg));
                    }
                }

                // Check for breakpoints BEFORE executing the instruction
                if self.breakpoints.contains(&current_pc)
                    && self.last_breakpoint_pc != Some(current_pc)
                {
                    self.at_breakpoint = true;
                    self.last_breakpoint_pc = Some(current_pc);
                    let line_number = self.get_line_for_pc(current_pc);
                    return Ok(DebugEvent::Breakpoint(current_pc, line_number));
                }

                let event = if self.interpreter.step() {
                    // Consume instruction cost after successful step
                    self.consume_instruction_cost();

                    let line_number = self.get_line_for_pc(current_pc);
                    DebugEvent::Step(current_pc, line_number)
                } else if let ProgramResult::Ok(result) = self.interpreter.vm.program_result {
                    self.consume_instruction_cost();
                    DebugEvent::Exit(result)
                } else if let ProgramResult::Err(err) = &self.interpreter.vm.program_result {
                    let error_msg = format!("Program error at PC 0x{:016x}: {:?}", current_pc, err);
                    DebugEvent::Error(error_msg)
                } else {
                    let error_msg = format!("Unknown program error at PC 0x{:016x}", current_pc);
                    DebugEvent::Error(error_msg)
                };
                return Ok(event);
            }
            DebugMode::Continue => loop {
                let current_pc = self.get_pc();

                // If we're at a breakpoint, execute the instruction and continue.
                if self.at_breakpoint {
                    if self.interpreter.step() {
                        // Consume instruction cost after successful step
                        self.consume_instruction_cost();

                        self.at_breakpoint = false;
                        self.last_breakpoint_pc = None; // Clear the last breakpoint PC.
                    } else if let ProgramResult::Ok(result) = self.interpreter.vm.program_result {
                        self.consume_instruction_cost();
                        return Ok(DebugEvent::Exit(result));
                    } else if let ProgramResult::Err(err) = &self.interpreter.vm.program_result {
                        let error_msg =
                            format!("Program error at PC 0x{:016x}: {:?}", current_pc, err);
                        return Ok(DebugEvent::Error(error_msg));
                    } else {
                        let error_msg =
                            format!("Unknown program error at PC 0x{:016x}", current_pc);
                        return Ok(DebugEvent::Error(error_msg));
                    }
                    continue;
                }

                // Check for breakpoints BEFORE executing the instruction.
                if self.breakpoints.contains(&current_pc)
                    && self.last_breakpoint_pc != Some(current_pc)
                {
                    // Stop at breakpoint without executing the instruction.
                    self.at_breakpoint = true;
                    self.last_breakpoint_pc = Some(current_pc);
                    let line_number = self.get_line_for_pc(current_pc);
                    return Ok(DebugEvent::Breakpoint(current_pc, line_number));
                }

                // Execute the instruction.
                if self.interpreter.step() {
                    // Consume instruction cost after successful step
                    self.consume_instruction_cost();
                } else if let ProgramResult::Ok(result) = self.interpreter.vm.program_result {
                    self.consume_instruction_cost();
                    return Ok(DebugEvent::Exit(result));
                } else if let ProgramResult::Err(err) = &self.interpreter.vm.program_result {
                    let error_msg = format!("Program error at PC 0x{:016x}: {:?}", current_pc, err);
                    return Ok(DebugEvent::Error(error_msg));
                } else {
                    let error_msg = format!("Unknown program error at PC 0x{:016x}", current_pc);
                    return Ok(DebugEvent::Error(error_msg));
                }
            },
        }
    }

    pub fn get_pc(&self) -> u64 {
        self.interpreter.reg[11] * ebpf::INSN_SIZE as u64
    }

    /// Check if DWARF line mapping is available
    pub fn has_line_mapping(&self) -> bool {
        self.dwarf_line_map.is_some()
    }

    /// Get debug information about the line mapping
    pub fn get_line_mapping_info(&self) -> String {
        if let Some(dwarf_map) = &self.dwarf_line_map {
            dwarf_map.debug_info()
        } else {
            "No DWARF line mapping available. Compile with debug info (-g)".to_string()
        }
    }

    /// Returns a slice of all register values.
    pub fn get_registers(&self) -> &[u64] {
        &self.interpreter.reg
    }

    /// Returns the value of a single register by index.
    pub fn get_register(&self, idx: usize) -> Option<u64> {
        self.interpreter.reg.get(idx).copied()
    }

    /// Sets the value of a register by index.
    pub fn set_register(&mut self, idx: usize, value: u64) -> Result<(), String> {
        if let Some(reg) = self.interpreter.reg.get_mut(idx) {
            *reg = value;
            Ok(())
        } else {
            Err(format!("Register index {} out of range", idx))
        }
    }

    pub fn get_rodata(&self) -> Option<&Vec<ROData>> {
        self.rodata.as_ref()
    }
}

impl<'a, 'b, C: ContextObject> DebuggerInterface for Debugger<'a, 'b, C> {
    fn step(&mut self) -> Value {
        self.set_debug_mode(DebugMode::Step);
        match self.run() {
            Ok(event) => match event {
                DebugEvent::Step(pc, line) => json!({
                    "type": "step",
                    "pc": pc,
                    "line": line
                }),
                DebugEvent::Breakpoint(pc, line) => json!({
                    "type": "breakpoint",
                    "pc": pc,
                    "line": line
                }),
                DebugEvent::Exit(code) => json!({
                    "type": "exit",
                    "code": code,
                    "compute_units": self.get_compute_units()
                }),
                DebugEvent::Error(msg) => json!({
                    "type": "error",
                    "message": msg
                }),
            },
            Err(e) => json!({
                "type": "error",
                "message": format!("{:?}", e)
            }),
        }
    }

    fn r#continue(&mut self) -> Value {
        self.set_debug_mode(DebugMode::Continue);
        match self.run() {
            Ok(event) => match event {
                DebugEvent::Step(pc, line) => json!({
                    "type": "step",
                    "pc": pc,
                    "line": line
                }),
                DebugEvent::Breakpoint(pc, line) => json!({
                    "type": "breakpoint",
                    "pc": pc,
                    "line": line
                }),
                DebugEvent::Exit(code) => json!({
                    "type": "exit",
                    "code": code
                }),
                DebugEvent::Error(msg) => json!({
                    "type": "error",
                    "message": msg
                }),
            },
            Err(e) => json!({
                "type": "error",
                "message": format!("{:?}", e)
            }),
        }
    }

    fn set_breakpoint(&mut self, file: String, line: usize) -> Value {
        match self.set_breakpoint_at_line(line) {
            Ok(()) => json!({
                "type": "setBreakpoint",
                "file": file,
                "line": line,
                "verified": true
            }),
            Err(e) => json!({
                "type": "setBreakpoint",
                "file": file,
                "line": line,
                "verified": false,
                "error": e
            }),
        }
    }

    fn remove_breakpoint(&mut self, file: String, line: usize) -> Value {
        match self.remove_breakpoint_at_line(line) {
            Ok(()) => json!({
                "type": "removeBreakpoint",
                "file": file,
                "line": line,
                "success": true
            }),
            Err(e) => json!({
                "type": "removeBreakpoint",
                "file": file,
                "line": line,
                "success": false,
                "error": e
            }),
        }
    }

    fn clear_breakpoints(&mut self, _file: String) -> Value {
        // Remove all line-based breakpoints.
        if let Some(dwarf_map) = &self.dwarf_line_map {
            let lines: Vec<usize> = self.line_breakpoints.iter().copied().collect();
            for line in lines {
                let pcs = dwarf_map.get_pcs_for_line(line);
                for pc in pcs {
                    self.breakpoints.remove(&pc);
                }
                self.line_breakpoints.remove(&line);
            }
        } else {
            self.breakpoints.clear();
            self.line_breakpoints.clear();
        }
        json!({"result": "ok"})
    }

    fn get_stack_frames(&self) -> Value {
        let vm = &self.interpreter.vm;
        let mut frames = Vec::new();
        let dwarf_map = self.dwarf_line_map.as_ref();
        let mut index = 0;

        // Helper to get function name, file, and line from PC.
        let lookup = |pc: u64| {
            if let Some(dwarf) = dwarf_map {
                // Try to get source location
                if let Some(loc) = dwarf.get_source_location(pc) {
                    let name = format!("{}", loc.file);
                    let file = loc.file.clone();
                    let line = loc.line as usize;
                    return (name, file, line);
                }
                // Fallback to just line..
                if let Some(line) = dwarf.get_line_for_pc(pc) {
                    return ("?".to_string(), "?".to_string(), line);
                }
            }
            ("?".to_string(), "?".to_string(), 0)
        };

        if vm.call_depth > 0 {
            for (_i, frame) in vm.call_frames[..vm.call_depth as usize].iter().enumerate() {
                let pc = frame.target_pc;
                let (name, file, line) = lookup(pc);
                frames.push(json!({
                    "index": index,
                    "name": name,
                    "file": file,
                    "line": line,
                    "instruction": pc
                }));
                index += 1;
            }
        }

        // Add the current frame (top of stack)
        let current_pc = self.get_pc();
        let (name, file, line) = lookup(current_pc);
        frames.push(json!({
            "index": index,
            "name": name,
            "file": file,
            "line": line,
            "instruction": current_pc
        }));

        json!({ "frames": frames })
    }

    fn get_registers(&self) -> Value {
        let registers = self.get_registers();
        let mut regs = Vec::new();

        for (i, &value) in registers.iter().enumerate() {
            regs.push(json!({
                "name": format!("r{}", i),
                "value": format!("0x{:016x}", value),
                "type": "u64"
            }));
        }

        json!({
            "registers": regs
        })
    }

    fn get_memory(&self, address: u64, size: usize) -> Value {
        // For now, return empty memory data
        // TODO: should probably read from input register
        json!({
            "address": address,
            "size": size,
            "data": []
        })
    }

    fn set_register(&mut self, index: usize, value: u64) -> Value {
        match self.set_register(index, value) {
            Ok(()) => json!({
                "type": "setRegister",
                "index": index,
                "value": value,
                "success": true
            }),
            Err(e) => json!({
                "type": "setRegister",
                "index": index,
                "value": value,
                "success": false,
                "error": e
            }),
        }
    }

    fn quit(&mut self) -> Value {
        json!({
            "type": "quit"
        })
    }

    fn get_rodata(&self) -> Value {
        if let Some(rodata_syms) = self.get_rodata() {
            let arr: Vec<_> = rodata_syms
                .iter()
                .map(|sym| {
                    json!({
                        "name": sym.name,
                        "address": format!("0x{:016x}", sym.address),
                        "value": sym.content,
                    })
                })
                .collect();
            json!({ "rodata": arr })
        } else {
            json!({ "rodata": [] })
        }
    }

    fn get_compute_units(&self) -> Value {
        let context = &self.interpreter.vm.context_object_pointer;
        let remaining = context.get_remaining();
        let total = self.initial_compute_budget;
        let used = total.saturating_sub(remaining);

        json!({
            "total": total,
            "used": used,
            "remaining": remaining,
        })
    }
}
