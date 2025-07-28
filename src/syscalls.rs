#![allow(clippy::arithmetic_side_effects)]
#![allow(clippy::too_many_arguments)]
// Copyright 2015 Big Switch Networks, Inc
//      (Algorithms for uBPF syscalls, originally in C)
// Copyright 2016 6WIND S.A. <quentin.monnet@6wind.com>
//      (Translation to Rust, other syscalls)
//
// Licensed under the Apache License, Version 2.0 <http://www.apache.org/licenses/LICENSE-2.0> or
// the MIT license <http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! This module implements some built-in syscalls that can be called from within an eBPF program.
//!
//! These syscalls may originate from several places:
//!
//! * Some of them mimic the syscalls available in the Linux kernel.
//! * Some of them were proposed as example syscalls in uBPF and they were adapted here.
//! * Other syscalls may be specific to sbpf.
//!
//! The prototype for syscalls is always the same: five `u64` as arguments, and a `u64` as a return
//! value. Hence some syscalls have unused arguments, or return a 0 value in all cases, in order to
//! respect this convention.

use crate::DebugContextObject;
use solana_sbpf::{
    declare_builtin_function,
    error::EbpfError,
    memory_region::{AccessType, MemoryMapping},
};
use std::{slice::from_raw_parts, str::from_utf8};


declare_builtin_function!(
    /// Prints a NULL-terminated UTF-8 string.
    SyscallLog,
    fn rust(
        _context_object: &mut DebugContextObject,
        vm_addr: u64,
        len: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let host_addr: Result<u64, EbpfError> =
            memory_mapping.map(AccessType::Load, vm_addr, len).into();
        let host_addr = host_addr?;
        unsafe {
            let c_buf = from_raw_parts(host_addr as *const u8, len as usize);
            let len = c_buf.iter().position(|c| *c == 0).unwrap_or(len as usize);
            let message = from_utf8(&c_buf[0..len]).unwrap_or("Invalid UTF-8 String");
            println!("Program log: {message}");
        }
        Ok(0)
    }
);

declare_builtin_function!(
    /// Prints the five arguments formated as u64 in decimal.
    SyscallLogU64,
    fn rust(
        _context_object: &mut DebugContextObject,
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        _memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        println!(
            "Program log: {:#x}, {:#x}, {:#x}, {:#x}, {:#x}",
            arg1, arg2, arg3, arg4, arg5
        );
        Ok(0)
    }
);

// TODO: Add more syscalls
