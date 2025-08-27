use crate::{
    adapter::DebuggerInterface,
    debugger::{DebugMode, Debugger},
};
use solana_sbpf::vm::ContextObject;
use std::io::{self, Write};

pub struct Repl<'a, 'b, C: ContextObject> {
    pub dbg: Debugger<'a, 'b, C>,
}

impl<'a, 'b, C: ContextObject> Repl<'a, 'b, C> {
    pub fn new(dbg: Debugger<'a, 'b, C>) -> Self {
        Self { dbg }
    }

    pub fn start(&mut self) {
        println!("\nsBPF Debugger REPL. Type 'help' for commands.");

        let stdin = io::stdin();
        loop {
            print!("dbg> ");
            io::stdout().flush().unwrap();
            let mut input = String::new();
            if stdin.read_line(&mut input).is_err() {
                break;
            }
            let cmd = input.trim();
            match cmd {
                "step" | "s" => {
                    self.dbg.set_debug_mode(DebugMode::Step);
                    match self.dbg.run() {
                        Ok(event) => match event {
                            crate::debugger::DebugEvent::Step(pc, line) => {
                                if let Some(line_num) = line {
                                    println!("Step at PC 0x{:016x} (line {})", pc, line_num);
                                } else {
                                    println!("Step at PC 0x{:016x}", pc);
                                }
                            }
                            crate::debugger::DebugEvent::Breakpoint(pc, line) => {
                                if let Some(line_num) = line {
                                    println!(
                                        "Breakpoint hit at PC 0x{:016x} (line {})",
                                        pc, line_num
                                    );
                                } else {
                                    println!("Breakpoint hit at PC 0x{:016x}", pc);
                                }
                            }
                            crate::debugger::DebugEvent::Exit(code) => {
                                println!("Program exited with code: {}", code);
                            }
                            crate::debugger::DebugEvent::Error(msg) => {
                                println!("Program error: {}", msg);
                            }
                        },
                        Err(e) => println!("Debugger error: {:?}", e),
                    }
                }
                "continue" | "c" => {
                    self.dbg.set_debug_mode(DebugMode::Continue);
                    match self.dbg.run() {
                        Ok(event) => match event {
                            crate::debugger::DebugEvent::Step(pc, line) => {
                                if let Some(line_num) = line {
                                    println!("Step at PC 0x{:016x} (line {})", pc, line_num);
                                } else {
                                    println!("Step at PC 0x{:016x}", pc);
                                }
                            }
                            crate::debugger::DebugEvent::Breakpoint(pc, line) => {
                                if let Some(line_num) = line {
                                    println!(
                                        "Breakpoint hit at PC 0x{:016x} (line {})",
                                        pc, line_num
                                    );
                                } else {
                                    println!("Breakpoint hit at PC 0x{:016x}", pc);
                                }
                            }
                            crate::debugger::DebugEvent::Exit(code) => {
                                println!("Program exited with code: {}", code);
                            }
                            crate::debugger::DebugEvent::Error(msg) => {
                                println!("Program error: {}", msg);
                            }
                        },
                        Err(e) => println!("Debugger error: {:?}", e),
                    }
                }
                cmd if cmd.starts_with("break ") => {
                    if let Some(arg) = cmd.split_whitespace().nth(1) {
                        // Try to parse as line number first
                        if let Ok(line) = arg.parse::<usize>() {
                            match self.dbg.set_breakpoint_at_line(line) {
                                Ok(()) => println!("Breakpoint set at line: {}", line),
                                Err(e) => println!("Error: {}", e),
                            }
                        } else if let Ok(pc) = arg.parse::<u64>() {
                            // Fall back to PC-based breakpoint
                            self.dbg.set_breakpoint(pc);
                            println!("Breakpoint set at instruction: {pc}");
                        } else {
                            println!(
                                "Error: Invalid breakpoint argument. Use line number or PC address."
                            );
                        }
                    }
                }
                cmd if cmd.starts_with("delete ") => {
                    if let Some(arg) = cmd.split_whitespace().nth(1) {
                        if let Ok(line) = arg.parse::<usize>() {
                            match self.dbg.remove_breakpoint_at_line(line) {
                                Ok(()) => println!("Breakpoint removed from line: {}", line),
                                Err(e) => println!("Error: {}", e),
                            }
                        } else {
                            println!("Error: Invalid line number for delete command.");
                        }
                    }
                }
                "info breakpoints" | "info b" => {
                    println!("{}", self.dbg.get_breakpoints_info());
                }
                "info line" => {
                    if let Some(line) = self.dbg.get_current_line() {
                        println!("Current line: {}", line);
                        let pcs = self.dbg.get_pcs_for_line(line);
                        if !pcs.is_empty() {
                            println!("Line {} maps to PCs: {:?}", line, pcs);
                        }
                    } else {
                        println!("No line information available for current PC");
                    }
                }
                "quit" => break,
                "help" => {
                    println!("Commands:");
                    println!("  step (s)                    - Execute one instruction");
                    println!("  continue (c)                 - Continue execution");
                    println!(
                        "  break <line|pc>              - Set breakpoint at line number or PC"
                    );
                    println!("  delete <line>                - Remove breakpoint at line");
                    println!("  info breakpoints (info b)    - Show all breakpoints");
                    println!("  info line                    - Show current line info");
                    println!("  info dwarf                   - Show DWARF debug info");
                    println!("  info dwarf-details           - Show detailed DWARF mapping info");
                    println!("  stack (bt)                   - Show call stack");
                    println!("  compute                      - Show compute unit information");
                    println!("  help                         - Show this help");
                    println!("  quit                         - Exit debugger");
                }
                "regs" => {
                    let regs = self.dbg.get_registers();
                    // ASCII table header
                    println!("+------------+--------------------+--------------------+");
                    println!("| Register   | Hex Value          | Decimal Value      |");
                    println!("+------------+--------------------+--------------------+");
                    for (i, val) in regs.iter().enumerate() {
                        println!(
                            "| {:<10} | {:<18} | {:>18} |",
                            format!("r{}", i),
                            format!("0x{:016x}", val),
                            val
                        );
                    }
                    println!("+------------+--------------------+--------------------+");
                }
                cmd if cmd.starts_with("reg ") => {
                    if let Some(arg) = cmd.split_whitespace().nth(1) {
                        if let Ok(idx) = arg.parse::<usize>() {
                            if let Some(val) = self.dbg.get_register(idx) {
                                println!(
                                    "+------------+--------------------+--------------------+"
                                );
                                println!(
                                    "| Register   | Hex Value          | Decimal Value      |"
                                );
                                println!(
                                    "+------------+--------------------+--------------------+"
                                );
                                println!(
                                    "| {:<10} | {:<18} | {:>18} |",
                                    format!("r{}", idx),
                                    format!("0x{:016x}", val),
                                    val
                                );
                                println!(
                                    "+------------+--------------------+--------------------+"
                                );
                            } else {
                                println!("Register index out of range");
                            }
                        } else {
                            println!("Invalid register index");
                        }
                    } else {
                        println!("Usage: reg <idx>");
                    }
                }
                cmd if cmd.starts_with("setreg ") => {
                    let mut parts = cmd.split_whitespace();
                    parts.next(); // skip 'setreg'
                    let idx_str = parts.next();
                    let val_str = parts.next();
                    if let (Some(idx_str), Some(val_str)) = (idx_str, val_str) {
                        if let Ok(idx) = idx_str.parse::<usize>() {
                            let value = if let Some(stripped) = val_str.strip_prefix("0x") {
                                u64::from_str_radix(stripped, 16)
                            } else {
                                val_str.parse::<u64>()
                            };
                            match value {
                                Ok(val) => match self.dbg.set_register(idx, val) {
                                    Ok(()) => println!("Set r{} = 0x{:016x} ({})", idx, val, val),
                                    Err(e) => println!("{}", e),
                                },
                                Err(_) => println!(
                                    "Invalid value: must be a number (decimal or 0x... hex)"
                                ),
                            }
                        } else {
                            println!("Invalid register index");
                        }
                    } else {
                        println!("Usage: setreg <idx> <value>");
                    }
                }
                "rodata" => {
                    if let Some(rodata_symbols) = self.dbg.get_rodata() {
                        println!(
                            "+---------------+----------------------+--------------------------+"
                        );
                        println!(
                            "| Symbol        | Address              | Value                    |"
                        );
                        println!(
                            "+---------------+----------------------+--------------------------+"
                        );
                        for symbol in rodata_symbols {
                            println!(
                                "| {:<13} | 0x{:016x}   | {:<24} |",
                                symbol.name, symbol.address, symbol.content
                            );
                        }
                        println!(
                            "+---------------+----------------------+--------------------------+"
                        );
                    } else {
                        println!("No .rodata information available");
                    }
                }
                "lines" => {
                    if let Some(ref dwarf_map) = self.dbg.dwarf_line_map {
                        println!("+----------+--------------------------+");
                        println!("| Line     | Instruction Addresses    |");
                        println!("+----------+--------------------------+");
                        let mut lines: Vec<_> = dwarf_map.get_line_to_addresses().iter().collect();
                        lines.sort_by_key(|(line, _)| *line);
                        for (line, pcs) in lines {
                            let pcs_str = pcs
                                .iter()
                                .map(|pc| format!("0x{:016x}", pc))
                                .collect::<Vec<_>>()
                                .join(", ");
                            println!("| {:<8} | {:<24} |", line, pcs_str);
                        }
                        println!("+----------+--------------------------+");
                    } else {
                        println!("No DWARF line mapping available.");
                    }
                }
                "stack" | "bt" => {
                    let stack = self.dbg.get_stack_frames();
                    if let Some(frames) = stack.get("frames").and_then(|f| f.as_array()) {
                        println!("Call stack:");
                        for frame in frames {
                            let idx = frame.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                            let name = frame.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            let file = frame.get("file").and_then(|v| v.as_str()).unwrap_or("?");
                            let line = frame.get("line").and_then(|v| v.as_u64()).unwrap_or(0);
                            let pc = frame
                                .get("instruction")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            println!("  #{idx}: {name} at {file}:{line} (PC 0x{pc:016x})");
                        }
                    } else {
                        println!("No stack frames available");
                    }
                }
                "compute" => {
                    let compute_data = self.dbg.get_compute_units();
                    if let Some(total) = compute_data.get("total").and_then(|v| v.as_u64()) {
                        if let Some(used) = compute_data.get("used").and_then(|v| v.as_u64()) {
                            println!("Program consumed {} of {} compute units", used, total);
                        }
                    }
                }
                _ => println!("Unknown command. Type 'help'."),
            }
        }
    }
}
