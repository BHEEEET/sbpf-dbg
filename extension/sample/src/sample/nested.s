// This is an example program that contains deeply nested function calls.

.globl entrypoint

entrypoint:
    call func_1
    call sol_log_64_
    exit
    
func_1:
    lddw r1, 0x1
    call func_2
    exit

func_2:
    lddw r2, 0x2
    call func_3
    exit

func_3:
    lddw r3, 0x3
    call func_4
    exit

func_4:
    lddw r4, 0x4
    call func_5
    exit

func_5:
    lddw r5, 0x5
    exit