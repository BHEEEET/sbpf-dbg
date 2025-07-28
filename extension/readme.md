# VS Code SBPF Debug

This extension provides debugging support for SBPF (Solana Berkeley Packet Filter) programs in Visual Studio Code.

**SBPF Debug** allows you to debug SBPF smart contracts and programs with features such as step, continue, breakpoints, variable inspection, and more.

## Features
- Launch and debug SBPF (.so) programs
- Set and hit breakpoints
- Step through code
- Inspect variables and memory
- View output and errors

## Getting Started

1. **Install the SBPF Debug extension in VS Code.**
2. **Prepare your SBPF program:**
   - Ensure you have a compiled `.so` file and (optionally) a debug `.o` file in your workspace.
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
      "program": "${workspaceFolder}/deploy/${workspaceFolderBasename}.so",
      "debugFile": "${workspaceFolder}/.sbpf/${workspaceFolderBasename}.o",
      "input": "0",
      "stopOnEntry": true
    }
  ]
}
```

4. **Start Debugging:**
   - Open the Run and Debug view in VS Code.
   - Select "SBPF Debug" as the environment.
   - Press the green play button to start debugging.

## Build and Run

- Clone the project.
- Open the project folder in VS Code.
- Press `F5` to build and launch SBPF Debug in a new VS Code window.
- Open your SBPF program file and set breakpoints as needed.

## Requirements
- Compiled SBPF program (`.so` file)
- (Optional) Debug info file (`.o` file)
- [sbpf-dbg](https://github.com/your-org/sbpf-dbg) debugger binary available on your system

## Contributing
Pull requests and issues are welcome!

## License
MIT