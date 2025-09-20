# VS Code sBPF Debugger

This extension provides debugging support for sBPF Assembly programs in Visual Studio Code.

## Features
- Set and hit breakpoints
- Step through code
- Inspect variables and memory
- Track Compute Units (CU)
- View output and errors

## Requirements
- sBPF assembly file (`.s` file)
- (Optional) Custom linker file (`.ld` file)
- [sbpf-dbg](https://github.com/bidhan-a/sbpf-dbg) debugger binary available on your system
- Solana platform tools

## Getting Started

1. **Install the sBPF Debug extension in VS Code.**
```
git clone https://github.com/bidhan-a/sbpf-dbg.git &&
cd sbpf-dbg/extension &&
npm i &&
npx @vscode/vsce package --no-yarn &&
code --install-extension sbpf-dbg-*.vsix
```
2. **Prepare your sBPF program:**
   - Ensure you have an assembly `.s` file in your workspace.
   - Optionally, provide a custom linker `.ld` file if needed.
3. **Configure your launch.json in `.vscode` folder:**
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
      "input": "${workspaceFolder}/src/.dbg/sample_input.hex",
      "stopOnEntry": true
    }
  ]
}
```

4. **Start Debugging:**
   - Open the Run and Debug view in VS Code.
   - Select "Debug" as the environment.
   - Press the green play button to start debugging.

