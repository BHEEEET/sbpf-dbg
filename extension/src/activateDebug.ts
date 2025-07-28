/*---------------------------------------------------------
 * Copyright (C) Microsoft Corporation. All rights reserved.
 *--------------------------------------------------------*/
/*
 * activateMockDebug.ts containes the shared extension code that can be executed both in node.js and the browser.
 */

"use strict";

import * as vscode from "vscode";
import {
  // WorkspaceFolder,
  // DebugConfiguration,
  ProviderResult,
  // CancellationToken,
} from "vscode";
import { SbpfDebugSession } from "./sbpfDebugSession";

export function activateDebug(
  context: vscode.ExtensionContext,
  factory?: vscode.DebugAdapterDescriptorFactory
) {
  // context.subscriptions.push(
  //   vscode.commands.registerCommand(
  //     "extension.sbpf-debug.runEditorContents",
  //     (resource: vscode.Uri) => {
  //       let targetResource = resource;
  //       if (!targetResource && vscode.window.activeTextEditor) {
  //         targetResource = vscode.window.activeTextEditor.document.uri;
  //       }
  //       if (targetResource) {
  //         vscode.debug.startDebugging(
  //           undefined,
  //           {
  //             type: "sbpf",
  //             name: "Run File",
  //             request: "launch",
  //             //   program: targetResource.fsPath,
  //           },
  //           { noDebug: true }
  //         );
  //       }
  //     }
  //   ),
  //   vscode.commands.registerCommand(
  //     "extension.sbpf-debug.debugEditorContents",
  //     (resource: vscode.Uri) => {
  //       let targetResource = resource;
  //       if (!targetResource && vscode.window.activeTextEditor) {
  //         targetResource = vscode.window.activeTextEditor.document.uri;
  //       }
  //       if (targetResource) {
  //         vscode.debug.startDebugging(undefined, {
  //           type: "sbpf",
  //           name: "Debug File",
  //           request: "launch",
  //           // program: targetResource.fsPath,
  //           stopOnEntry: true,
  //         });
  //       }
  //     }
  //   ),
  //   vscode.commands.registerCommand(
  //     "extension.sbpf-debug.toggleFormatting",
  //     (variable) => {
  //       const ds = vscode.debug.activeDebugSession;
  //       if (ds) {
  //         ds.customRequest("toggleFormatting");
  //       }
  //     }
  //   )
  // );

  // context.subscriptions.push(
  //   vscode.commands.registerCommand(
  //     "extension.sbpf-debug.getProgramName",
  //     (config) => {
  //       return vscode.window.showInputBox({
  //         placeHolder:
  //           "Please enter the name of a program file in the workspace folder",
  //         value: "program.so",
  //       });
  //     }
  //   )
  // );

  // // register a configuration provider for 'sbpf' debug type
  // const provider = new SbpfConfigurationProvider();
  // context.subscriptions.push(
  //   vscode.debug.registerDebugConfigurationProvider("sbpf", provider)
  // );

  // // register a dynamic configuration provider for 'sbpf' debug type
  // context.subscriptions.push(
  //   vscode.debug.registerDebugConfigurationProvider(
  //     "sbpf",
  //     {
  //       provideDebugConfigurations(
  //         folder: WorkspaceFolder | undefined
  //       ): ProviderResult<DebugConfiguration[]> {
  //         return [
  //           {
  //             name: "Dynamic Launch",
  //             request: "launch",
  //             type: "sbpf",
  //             program: "${file}",
  //           },
  //           {
  //             name: "Another Dynamic Launch",
  //             request: "launch",
  //             type: "sbpf",
  //             program: "${file}",
  //           },
  //           {
  //             name: "Mock Launch",
  //             request: "launch",
  //             type: "sbpf",
  //             program: "${file}",
  //           },
  //         ];
  //       },
  //     },
  //     vscode.DebugConfigurationProviderTriggerKind.Dynamic
  //   )
  // );

  if (!factory) {
    factory = new InlineDebugAdapterFactory();
  }
  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("sbpf", factory)
  );
  if ("dispose" in factory) {
    context.subscriptions.push(factory as vscode.Disposable);
  }
}

// class SbpfConfigurationProvider implements vscode.DebugConfigurationProvider {
//   /**
//    * Massage a debug configuration just before a debug session is being launched,
//    * e.g. add all missing attributes to the debug configuration.
//    */
//   resolveDebugConfiguration(
//     folder: WorkspaceFolder | undefined,
//     config: DebugConfiguration,
//     token?: CancellationToken
//   ): ProviderResult<DebugConfiguration> {
//     // if launch.json is missing or empty
//     if (!config.type && !config.request && !config.name) {
//       const editor = vscode.window.activeTextEditor;
//       if (editor) {
//         config.type = "sbpf";
//         config.name = "Launch";
//         config.request = "launch";
//         config.program = "${file}";
//         config.stopOnEntry = true;
//       }
//     }

//     if (!config.program) {
//       return vscode.window
//         .showInformationMessage("Cannot find a program to debug")
//         .then((_) => {
//           return undefined; // abort launch
//         });
//     }

//     return config;
//   }
// }

class InlineDebugAdapterFactory
  implements vscode.DebugAdapterDescriptorFactory
{
  createDebugAdapterDescriptor(
    _session: vscode.DebugSession
  ): ProviderResult<vscode.DebugAdapterDescriptor> {
    return new vscode.DebugAdapterInlineImplementation(new SbpfDebugSession());
  }
}
