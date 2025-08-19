/*---------------------------------------------------------
 * sBPF Runtime - Interface to Rust sBPF Debugger
 *--------------------------------------------------------*/

import { EventEmitter } from "events";
import { spawn, ChildProcess } from "child_process";

let nextRequestId = 1;

export interface ISbpfBreakpoint {
  id: number;
  line: number;
  verified: boolean;
  pc?: number;
}

export interface ISbpfStackFrame {
  index: number;
  name: string;
  file: string;
  line: number;
  column?: number;
  instruction?: number;
  pc?: number;
}

export interface ISbpfVariable {
  name: string;
  value: string;
  type: string;
  reference?: number;
}

export interface ISbpfRegister {
  name: string;
  value: number;
  hexValue: string;
}

export interface ISbpfMemoryRegion {
  address: number;
  size: number;
  data: Uint8Array;
}

export interface ISbpfDebugInfo {
  hasDwarf: boolean;
  sourceFiles: string[];
  functions: Array<{ name: string; address: number }>;
}

export interface ISbpfLaunchConfig {
  program: string; // Path to .so file
  debugFile?: string; // Path to .o file with debug info
  input?: string; // Program input (file or hex string)
  heap?: number; // Heap size in bytes
  maxInstructions?: number; // Max instructions to execute
  stopOnEntry?: boolean; // Whether to stop at entry point
}

export interface ISbpfCommand {
  command: string;
  args?: any[];
  requestId?: number;
}

export interface ISbpfResponse {
  success: boolean;
  data?: any;
  error?: string;
  requestId?: number;
}

export class SbpfRuntime extends EventEmitter {
  private debuggerProcess: ChildProcess | undefined;
  private pendingRequests: Map<number, (resp: ISbpfResponse) => void> =
    new Map();
  private buffer = "";
  private _breakpointQueue: (() => Promise<void>)[] = [];
  private _backendReady: boolean = false;

  constructor() {
    super();
  }

  public async start(config: ISbpfLaunchConfig): Promise<void> {
    return new Promise((resolve, reject) => {
      const args = [
        "--file",
        config.program,
        "--input",
        config.input || "0",
        "--heap",
        (config.heap || 0).toString(),
        "--max-ixs",
        (config.maxInstructions || 10000).toString(),
        "--adapter",
      ];
      if (config.debugFile) {
        args.push("--debug-file", config.debugFile);
      }
      this.debuggerProcess = spawn("sbpf-dbg", args, {
        stdio: ["pipe", "pipe", "pipe"],
      });
      if (!this.debuggerProcess.stdout || !this.debuggerProcess.stdin) {
        reject(new Error("Failed to create debugger process"));
        return;
      }
      this.debuggerProcess.stdout.on("data", (data) => {
        this.handleDebuggerOutput(data.toString());
      });
      this.debuggerProcess.stderr?.on("data", (data) => {
        // Only show stderr output for process-level failures, not as debugger errors
        const errorMsg = data.toString().trim();
        if (errorMsg) {
          // Optionally, emit as output for user info, but do NOT emit as 'error' for debugger logic
          this.emit("output", "stderr", errorMsg);
        }
      });
      this.debuggerProcess.on("close", (code) => {
        if (typeof code === "number" && code !== 0) {
          this.emit(
            "error",
            new Error(`Debugger process exited with code ${code}`)
          );
        }
        this.emit("exit");
      });
      this.debuggerProcess.on("error", (error) => {
        this.emit("error", error);
      });

      setTimeout(async () => {
        if (config.stopOnEntry) {
          this.emit("entry");
        } else {
          this.continue();
        }
        this._backendReady = true;
        // Flush queued breakpoint operations
        for (const op of this._breakpointQueue) {
          await op();
        }
        this._breakpointQueue = [];
        resolve();
      }, 1000);
    });
  }

  private handleDebuggerOutput(output: string): void {
    this.buffer += output;
    let newlineIdx;
    while ((newlineIdx = this.buffer.indexOf("\n")) !== -1) {
      const line = this.buffer.slice(0, newlineIdx).trim();
      this.buffer = this.buffer.slice(newlineIdx + 1);
      if (!line) {
        continue;
      }
      if (line.startsWith("Program log:")) {
        // Emit a custom 'output' event for log lines
        const logMsg = line.trim();
        this.emit("output", "stdout", logMsg);
        continue;
      }
      if (line.startsWith("error:")) {
        // Handle error messages from the backend
        const errorMsg = line.substring(6).trim();
        this.emit("error", new Error(`Runtime error: ${errorMsg}`));
        this.emit("output", "stderr", errorMsg);
        continue;
      }
      let event: ISbpfResponse;
      try {
        event = JSON.parse(line);
      } catch (e) {
        continue;
      }

      // Check for error responses from the backend
      if (event.success === false) {
        const errorMsg = event.error || "Unknown error from debugger backend";
        this.emit("error", new Error(errorMsg));
        continue;
      }

      // Check for error or exit events in the response data
      if (
        event.data &&
        typeof event.data === "object" &&
        "type" in event.data
      ) {
        const data = event.data as any;
        if (data.type === "exit") {
          this.emit(
            "output",
            "stdout",
            `Program exited with code: ${event.data.code}`
          );
          // Log compute units usage
          if (data.compute_units) {
            this.emit(
              "output",
              "stdout",
              `Program consumed ${data.compute_units.used} of ${data.compute_units.total} compute units`
            );
          }
          this.emit("exit");
        } else if (data.type === "error") {
          const errorMsg = data.message || "Runtime error occurred";
          this.emit("error", new Error(errorMsg));
          continue;
        }
      }

      if (event.requestId && this.pendingRequests.has(event.requestId)) {
        const cb = this.pendingRequests.get(event.requestId)!;
        this.pendingRequests.delete(event.requestId);
        cb(event);
      } else {
        this.emit("event", event);
      }
    }
  }

  private sendCommand(cmd: ISbpfCommand): Promise<ISbpfResponse> {
    if (!this.debuggerProcess || !this.debuggerProcess.stdin) {
      return Promise.reject(new Error("Debugger not connected"));
    }
    const requestId = nextRequestId++;
    cmd.requestId = requestId;
    const commandStr = JSON.stringify(cmd) + "\n";
    return new Promise((resolve, reject) => {
      this.pendingRequests.set(requestId, (response) => {
        if (response.success === false) {
          reject(new Error(response.error || "Command failed"));
        } else {
          resolve(response);
        }
      });
      this.debuggerProcess!.stdin!.write(commandStr);
    });
  }

  public async continue(): Promise<void> {
    await this.sendCommand({ command: "continue" });
  }
  public async step(): Promise<void> {
    await this.sendCommand({ command: "step" });
  }
  public async clearBreakpoints(file: string): Promise<void> {
    if (!this._backendReady) {
      this._breakpointQueue.push(() => this.clearBreakpoints(file));
      return;
    }
    await this.sendCommand({ command: "clearBreakpoints", args: [file] });
  }

  public async setBreakpoint(
    file: string,
    line: number
  ): Promise<ISbpfBreakpoint> {
    if (!this._backendReady) {
      return new Promise((resolve, reject) => {
        this._breakpointQueue.push(async () => {
          try {
            const result = await this.setBreakpoint(file, line);
            resolve(result);
          } catch (e) {
            reject(e);
          }
        });
      });
    }
    const resp = await this.sendCommand({
      command: "setBreakpoint",
      args: [file, line],
    });
    if (resp.success && resp.data) {
      return resp.data;
    }
    throw new Error(resp.error || "Failed to set breakpoint");
  }

  public async getStackFrames(): Promise<ISbpfStackFrame[]> {
    const resp = await this.sendCommand({ command: "getStackFrames" });
    if (resp.success && resp.data) {
      return resp.data.frames;
    }
    return [];
  }

  public async getRegisters(): Promise<ISbpfRegister[]> {
    const resp = await this.sendCommand({ command: "getRegisters" });
    if (resp.success && resp.data) {
      return resp.data.registers;
    }
    return [];
  }

  public async getRodata(): Promise<ISbpfVariable[]> {
    const resp = await this.sendCommand({ command: "getRodata" });
    if (resp.success && resp.data) {
      return resp.data.rodata;
    }
    return [];
  }

  public async getMemory(
    address: number,
    size: number
  ): Promise<ISbpfMemoryRegion> {
    const resp = await this.sendCommand({
      command: "getMemory",
      args: [address, size],
    });
    if (resp.success && resp.data) {
      return resp.data;
    }
    throw new Error(resp.error || "Failed to get memory");
  }
  public async setRegister(index: number, value: number): Promise<void> {
    await this.sendCommand({ command: "setRegister", args: [index, value] });
  }
  public async getDebugInfo(): Promise<ISbpfDebugInfo> {
    const resp = await this.sendCommand({ command: "getDebugInfo" });
    if (resp.success && resp.data) {
      return resp.data;
    }
    return { hasDwarf: false, sourceFiles: [], functions: [] };
  }

  public async getComputeUnits(): Promise<{
    total: number;
    used: number;
    remaining: number;
  }> {
    const resp = await this.sendCommand({ command: "getComputeUnits" });
    if (resp.success && resp.data) {
      const d = resp.data as any;
      return {
        total: typeof d.total === "number" ? d.total : 0,
        used: typeof d.used === "number" ? d.used : 0,
        remaining: typeof d.remaining === "number" ? d.remaining : 0,
      };
    }
    return { total: 0, used: 0, remaining: 0 };
  }
  public async shutdown(): Promise<void> {
    if (this.debuggerProcess) {
      await this.sendCommand({ command: "quit" });
      this.debuggerProcess.kill();
      this.debuggerProcess = undefined;
    }
  }
}
