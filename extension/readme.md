# VS Code sBPF Debugger

This extension provides debugging support for sBPF Assembly programs in Visual Studio Code.

## Features
- Set and hit breakpoints
- Step through code
- Inspect variables and memory
- Track Compute Units (CU)
- View output and errors

## Getting Started

1. **Install the sBPF Debug extension in VS Code.**
2. **Prepare your sBPF program:**
   - Ensure you have an assembly `.s` file in your workspace.
   - Optionally, provide a custom linker `.ld` file if needed.
3. **Configure your launch.json:**
   - Use the following example configuration:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "sbpf",
      "request": "launch",
      "name": "Debug SBPF Program",
      "program": "${workspaceFolder}/src/${workspaceFolderBasename}/${workspaceFolderBasename}.s",
      "input": "0",
      "stopOnEntry": true
    }
  ]
}
```

4. **Start Debugging:**
   - Open the Run and Debug view in VS Code.
   - Select "Debug" as the environment.
   - Press the green play button to start debugging.

## Build and Run

- Clone the project.
- Open the project folder in VS Code.
- Press `F5` to build and launch SBPF Debug in a new VS Code window.
- Open your SBPF assembly file and set breakpoints as needed.

## Requirements
- sBPF assembly file (`.s` file)
- (Optional) Custom linker file (`.ld` file)
- [sbpf-dbg](https://github.com/bidhan-a/sbpf-dbg) debugger binary available on your system
- Solana platform tools
