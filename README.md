# sBPF Debugger

An interactive debugger for Solana sBPF programs.

## Features

- Execution control (step/continue)
- Breakpoint support
- Register inspection and modification
- Error handling
- REPL interface
- VSCode debug adapter


## Installation

```bash
cargo install --git https://github.com/bidhan-a/sbpf-dbg
```

### sbpf

The debugger works best with projects built using [sbpf](https://github.com/deanmlittle/sbpf).

Note: To enable debugging, the assembly must be compiled with debug information (`sbpf build --debug`). This functionality is not yet available in the official sbpf repo, but is supported in this [fork](https://github.com/bidhan-a/sbpf-cli/tree/feat/debug).

## Usage

### Example
```bash
sbpf-dbg -f test_programs/test.so -d test_programs/test.o
```

### Command Line Options
- `-f, --file <FILE>`: Path to the program ELF (.so)
- `-d, --debug-file <DEBUG_FILE>`: Path to the debug info file (.o)
- `--input <BYTES>`: Program input bytes (default: "0")

## REPL

Once the debugger starts, you'll see a `dbg>` prompt. Here are the available commands:

### Execution Control
| Command | Alias | Description |
|---------|-------|-------------|
| `step` | `s` | Execute one instruction |
| `continue` | `c` | Continue execution until breakpoint or exit |

### Breakpoints
| Command | Description |
|---------|-------------|
| `lines` | Show lines |
| `break <line>` | Set breakpoint at line number |
| `delete <line>` | Remove breakpoint at line |
| `info breakpoints` | Show all breakpoints |

### Register Operations
| Command | Description |
|---------|-------------|
| `regs` | Display all registers in table format |
| `reg <idx>` | Display specific register. |
| `setreg <idx> <value>` | Set register value (supports hex with 0x prefix) |

### Utility
| Command | Description |
|---------|-------------|
| `help` | Show command help |
| `quit` | Exit debugger |


## VSCode Debugger
The VSCode debugger extension is inside the `extension` directory. 

### Demo

[VSCode Debugger](docs/vscode-debugger.gif)

-----

## TODO
- Handle accounts input
- Track compute units usage
- Add more syscalls
- ...

