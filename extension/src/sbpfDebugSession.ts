import {
  LoggingDebugSession,
  InitializedEvent,
  StoppedEvent,
  TerminatedEvent,
  StackFrame,
  Breakpoint,
  Source,
  Thread,
  OutputEvent,
  Scope,
  Handles,
} from "@vscode/debugadapter";
import { Subject } from "await-notify";
import { DebugProtocol } from "@vscode/debugprotocol";
import { SbpfRuntime, ISbpfLaunchConfig } from "./sbpfRuntime";

/**
 * This interface describes the mock-debug specific launch attributes
 * (which are not part of the Debug Adapter Protocol).
 * The schema for these attributes lives in the package.json of the mock-debug extension.
 * The interface should always match this schema.
 */
interface ILaunchRequestArguments extends DebugProtocol.LaunchRequestArguments {
  /** An absolute path to the "program" (.so) file. */
  program: string;
  /** An aboslute path to the debug (.o) file.  */
  debugFile: string;
  /** Program input. */
  input: string;
  /** Automatically stop target after launch. If not specified, target does not stop. */
  stopOnEntry?: boolean;
  /** enable logging the Debug Adapter Protocol */
  trace?: boolean;
  /** run without debugging */
  noDebug?: boolean;
  /** if specified, results in a simulated compile error in launch. */
  compileError?: "default" | "show" | "hide";
}

interface IAttachRequestArguments extends ILaunchRequestArguments {}

export class SbpfDebugSession extends LoggingDebugSession {
  private static threadID = 1;
  private _runtime: SbpfRuntime;
  private _configurationDone = new Subject();
  private _variableHandles = new Handles<string>();
  private _runtimeReady: Promise<void> = Promise.resolve();

  public constructor() {
    super("sbpf-debug.txt");
    this.setDebuggerLinesStartAt1(false);
    this.setDebuggerColumnsStartAt1(false);
    this._runtime = new SbpfRuntime();

    this._runtime.on("entry", () => {
      this.sendEvent(new StoppedEvent("entry", SbpfDebugSession.threadID));
    });

    this._runtime.on("exit", () => {
      this.sendEvent(new TerminatedEvent());
    });

    this._runtime.on("output", (type: string, text: string) => {
      const category = type === "stderr" ? "stderr" : "stdout";
      // Check if this is a JSON exit event from the backend
      try {
        const maybeJson = JSON.parse(text);
        if (
          maybeJson &&
          maybeJson.type === "exit" &&
          typeof maybeJson.code !== "undefined"
        ) {
          // Optionally, show exit code to user
          this.sendEvent(
            new OutputEvent(
              `Program exited with code: ${maybeJson.code}\n`,
              "stdout"
            )
          );
          // Terminate the debug session
          this.sendEvent(new TerminatedEvent());
          return;
        }
      } catch (e) {
        // Not JSON, just regular output
      }
      const e: DebugProtocol.OutputEvent = new OutputEvent(
        `${text}\n`,
        category
      );
      this.sendEvent(e);
    });

    // Handle runtime errors
    this._runtime.on("error", (error: Error) => {
      // Log the error
      console.error("Runtime error:", error.message);

      // Send error output to the debug console
      this.sendEvent(new OutputEvent(`Error: ${error.message}\n`, "stderr"));

      // Send a stopped event with error information
      this.sendEvent(new StoppedEvent("exception", SbpfDebugSession.threadID));
    });
  }

  protected async initializeRequest(
    response: DebugProtocol.InitializeResponse,
    args: DebugProtocol.InitializeRequestArguments
  ): Promise<void> {
    // build and return the capabilities of this debug adapter:
    response.body = response.body || {};

    // the adapter implements the configurationDone request.
    response.body.supportsConfigurationDoneRequest = true;

    // make VS Code use 'evaluate' when hovering over source
    response.body.supportsEvaluateForHovers = true;

    // make VS Code show a 'step back' button
    response.body.supportsStepBack = false;

    // make VS Code support data breakpoints
    response.body.supportsDataBreakpoints = true;

    // make VS Code support completion in REPL
    response.body.supportsCompletionsRequest = true;
    response.body.completionTriggerCharacters = [".", "["];

    // make VS Code send cancel request
    response.body.supportsCancelRequest = true;

    // make VS Code send the breakpointLocations request
    response.body.supportsBreakpointLocationsRequest = true;

    // make VS Code provide "Step in Target" functionality
    response.body.supportsStepInTargetsRequest = false;

    // the adapter defines two exceptions filters, one with support for conditions.
    response.body.supportsExceptionFilterOptions = true;
    response.body.exceptionBreakpointFilters = [
      {
        filter: "namedException",
        label: "Named Exception",
        description: `Break on named exceptions. Enter the exception's name as the Condition.`,
        default: false,
        supportsCondition: true,
        conditionDescription: `Enter the exception's name`,
      },
      {
        filter: "otherExceptions",
        label: "Other Exceptions",
        description: "This is a other exception",
        default: true,
        supportsCondition: false,
      },
    ];

    // make VS Code send exceptionInfo request
    response.body.supportsExceptionInfoRequest = true;

    // make VS Code send setVariable request
    response.body.supportsSetVariable = true;

    // make VS Code send setExpression request
    response.body.supportsSetExpression = true;

    // make VS Code send disassemble request
    response.body.supportsDisassembleRequest = true;
    response.body.supportsSteppingGranularity = true;
    response.body.supportsInstructionBreakpoints = true;

    // make VS Code able to read and write variable memory
    response.body.supportsReadMemoryRequest = true;
    response.body.supportsWriteMemoryRequest = true;

    response.body.supportSuspendDebuggee = true;
    response.body.supportTerminateDebuggee = true;
    response.body.supportsFunctionBreakpoints = true;
    response.body.supportsDelayedStackTraceLoading = true;

    this.sendResponse(response);

    // since this debug adapter can accept configuration requests like 'setBreakpoint' at any time,
    // we request them early by sending an 'initializeRequest' to the frontend.
    // The frontend will end the configuration sequence by calling 'configurationDone' request.
    this.sendEvent(new InitializedEvent());
  }

  /**
   * Called at the end of the configuration sequence.
   * Indicates that all breakpoints etc. have been sent to the DA and that the 'launch' can start.
   */
  protected configurationDoneRequest(
    response: DebugProtocol.ConfigurationDoneResponse,
    args: DebugProtocol.ConfigurationDoneArguments
  ): void {
    super.configurationDoneRequest(response, args);
    // notify the launchRequest that configuration has finished
    this._configurationDone.notify();
  }

  protected async attachRequest(
    response: DebugProtocol.AttachResponse,
    args: IAttachRequestArguments
  ) {
    return this.launchRequest(response, args);
  }

  protected async launchRequest(
    response: DebugProtocol.LaunchResponse,
    args: ILaunchRequestArguments
  ): Promise<void> {
    try {
      // wait 1 second until configuration has finished (and configurationDoneRequest has been called)
      await this._configurationDone.wait(1000);

      const config: ISbpfLaunchConfig = {
        program: args.program,
        debugFile: args.debugFile,
        input: args.input,
        stopOnEntry: args.stopOnEntry,
        heap: "heap" in args && args.heap ? parseInt(args.heap as any) : 0,
        maxInstructions:
          "maxInstructions" in args && args.maxInstructions
            ? parseInt(args.maxInstructions as any)
            : 10000,
      };

      this._runtimeReady = this._runtime.start(config);
      await this._runtimeReady;
      this.sendResponse(response);
    } catch (error) {
      // Handle launch errors
      const errorMessage =
        error instanceof Error ? error.message : "Unknown launch error";
      this.sendErrorResponse(response, {
        id: 1001,
        format: `Failed to launch debugger: ${errorMessage}`,
        showUser: true,
      });
    }
  }

  protected async setBreakPointsRequest(
    response: DebugProtocol.SetBreakpointsResponse,
    args: DebugProtocol.SetBreakpointsArguments
  ): Promise<void> {
    try {
      console.log("setBreakPointsRequest", args);
      await this._runtimeReady;
      const path = args.source.path as string;
      const clientLines = args.lines || [];

      // Clear all breakpoints for this file if supported
      if ("clearBreakpointsForFile" in this._runtime) {
        await (this._runtime as any).clearBreakpointsForFile(path);
      } else if ("clearBreakpoints" in this._runtime) {
        await (this._runtime as any).clearBreakpoints(path);
      }

      // Set and verify breakpoint locations
      const breakpoints: DebugProtocol.Breakpoint[] = [];
      for (const line of clientLines) {
        try {
          await this._runtime.setBreakpoint(path, line);
          breakpoints.push(new Breakpoint(true, line));
        } catch (e) {
          const errorMsg =
            e instanceof Error ? e.message : "Unknown breakpoint error";
          breakpoints.push(new Breakpoint(false, line));
        }
      }

      response.body = { breakpoints };
      this.sendResponse(response);
    } catch (error) {
      const errorMessage =
        error instanceof Error
          ? error.message
          : "Unknown error setting breakpoints";
      this.sendErrorResponse(response, {
        id: 1004,
        format: `Failed to set breakpoints: ${errorMessage}`,
        showUser: true,
      });
    }
  }

  protected async continueRequest(
    response: DebugProtocol.ContinueResponse,
    args: DebugProtocol.ContinueArguments
  ): Promise<void> {
    try {
      await this._runtime.continue();
      this.sendResponse(response);
      this.sendEvent(new StoppedEvent("breakpoint", SbpfDebugSession.threadID));
    } catch (error) {
      const errorMessage =
        error instanceof Error
          ? error.message
          : "Unknown error during continue";
      this.sendErrorResponse(response, {
        id: 1002,
        format: `Continue failed: ${errorMessage}`,
        showUser: true,
      });
    }
  }

  protected async nextRequest(
    response: DebugProtocol.NextResponse,
    args: DebugProtocol.NextArguments
  ): Promise<void> {
    try {
      await this._runtime.step();
      this.sendResponse(response);
      this.sendEvent(new StoppedEvent("step", SbpfDebugSession.threadID));
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : "Unknown error during step";
      this.sendErrorResponse(response, {
        id: 1003,
        format: `Step failed: ${errorMessage}`,
        showUser: true,
      });
    }
  }

  protected stepInRequest(
    response: DebugProtocol.StepInResponse,
    args: DebugProtocol.StepInArguments
  ): void {
    // Noop
    this.sendResponse(response);
    this.sendEvent(new StoppedEvent("step", SbpfDebugSession.threadID));
  }

  protected stepOutRequest(
    response: DebugProtocol.StepOutResponse,
    args: DebugProtocol.StepOutArguments
  ): void {
    // Noop
    this.sendResponse(response);
    this.sendEvent(new StoppedEvent("step", SbpfDebugSession.threadID));
  }

  protected threadsRequest(response: DebugProtocol.ThreadsResponse): void {
    // runtime supports no threads so just return a default thread.
    response.body = {
      threads: [
        new Thread(SbpfDebugSession.threadID, "thread 1"),
        // new Thread(SbpfDebugSession.threadID + 1, "thread 2"),
      ],
    };
    this.sendResponse(response);
  }

  protected async stackTraceRequest(
    response: DebugProtocol.StackTraceResponse,
    args: DebugProtocol.StackTraceArguments
  ): Promise<void> {
    const frames = await this._runtime.getStackFrames();
    response.body = {
      stackFrames: frames.map(
        (f) =>
          new StackFrame(
            f.index,
            f.name,
            new Source(
              f.file ? f.file.split(/[\\/]/).pop() || f.file : "program",
              f.file
            ),
            f.line,
            f.column
          )
      ),
      totalFrames: frames.length,
    };
    this.sendResponse(response);
  }

  protected scopesRequest(
    response: DebugProtocol.ScopesResponse,
    args: DebugProtocol.ScopesArguments
  ): void {
    response.body = {
      scopes: [
        new Scope("Registers", this._variableHandles.create("registers"), true),
        new Scope("Rodata", this._variableHandles.create("rodata"), true),
        new Scope(
          "Compute Units",
          this._variableHandles.create("compute"),
          true
        ),
      ],
    };
    this.sendResponse(response);
  }

  protected async variablesRequest(
    response: DebugProtocol.VariablesResponse,
    args: DebugProtocol.VariablesArguments
  ): Promise<void> {
    let vars: any[] = [];
    const v = this._variableHandles.get(args.variablesReference);
    if (v === "registers") {
      vars = (await this._runtime.getRegisters()) || [];
    } else if (v === "rodata") {
      vars = (await this._runtime.getRodata()) || [];
    } else if (v === "compute") {
      const cu = await this._runtime.getComputeUnits();
      vars = [
        { name: "Total", value: cu.total.toString(), type: "u64" },
        { name: "Used", value: cu.used.toString(), type: "u64" },
        { name: "Remaining", value: cu.remaining.toString(), type: "u64" },
      ];
    } else {
      vars = [];
    }
    response.body = {
      variables: vars.map((v) => ({
        name: v.name,
        value: v.value,
        type: v.type,
        variablesReference: 0,
      })),
    };
    this.sendResponse(response);
  }

  protected setVariableRequest(
    response: DebugProtocol.SetVariableResponse,
    args: DebugProtocol.SetVariableArguments
  ): void {
    const v = this._variableHandles.get(args.variablesReference);
    if (v === "registers") {
      // Register name is like 'r0', 'r1', ...
      const match = /^r(\d+)$/.exec(args.name);
      if (match) {
        const regIndex = parseInt(match[1], 10);
        // Try to parse value as hex or decimal
        let value = args.value.trim();
        let numValue: number | undefined = undefined;
        if (value.startsWith("0x") || value.startsWith("0X")) {
          numValue = parseInt(value, 16);
        } else {
          numValue = parseInt(value, 10);
        }
        if (!isNaN(numValue)) {
          this._runtime
            .setRegister(regIndex, numValue)
            .then(() => {
              response.body = {
                value: `0x${numValue!.toString(16)}`,
                type: "u64",
                variablesReference: 0,
              };
              this.sendResponse(response);
            })
            .catch((err: any) => {
              this.sendErrorResponse(response, {
                id: 1004,
                format: `Failed to set register: ${err}`,
                showUser: true,
              });
            });
          return;
        } else {
          this.sendErrorResponse(response, {
            id: 1005,
            format: `Invalid value for register: ${args.value}`,
            showUser: true,
          });
          return;
        }
      } else {
        this.sendErrorResponse(response, {
          id: 1006,
          format: `Invalid register name: ${args.name}`,
          showUser: true,
        });
        return;
      }
    } else if (v === "rodata") {
      this.sendErrorResponse(response, {
        id: 1007,
        format: `Cannot set value of .rodata symbol`,
        showUser: true,
      });
      return;
    } else {
      this.sendErrorResponse(response, {
        id: 1008,
        format: `Cannot set value of this variable`,
        showUser: true,
      });
      return;
    }
  }

  protected async disconnectRequest(
    response: DebugProtocol.DisconnectResponse,
    args: DebugProtocol.DisconnectArguments
  ): Promise<void> {
    await this._runtime.shutdown();
    this.sendResponse(response);
  }

  protected exceptionInfoRequest(
    response: DebugProtocol.ExceptionInfoResponse,
    args: DebugProtocol.ExceptionInfoArguments
  ): void {
    // Provide detailed exception information
    response.body = {
      exceptionId: "sbpf-runtime-error",
      description: "An error occurred during program execution",
      breakMode: "always",
      details: {
        message: "The SBPF program encountered an error during execution",
        typeName: "SBPFRuntimeError",
        stackTrace: "See the debug console for detailed error information",
      },
    };
    this.sendResponse(response);
  }
}
