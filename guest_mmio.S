// Guest code for MMIO test
// This will be assembled to generate correct ARM64 instructions
    .section .text
    .global guest_mmio_start
    .global guest_mmio_end

guest_mmio_start:
    // UART base address: 0x09000000
    mov     x19, #0x9000000
    
    // Write 'M' (0x4D) to UART
    mov     w1, #0x4D
    str     w1, [x19]
    
    // Write 'M' (0x4D) again
    mov     w1, #0x4D
    str     w1, [x19]
    
    // Write 'I' (0x49)
    mov     w1, #0x49
    str     w1, [x19]
    
    // Write 'O' (0x4F)
    mov     w1, #0x4F
    str     w1, [x19]
    
    // Write newline (0x0A)
    mov     w1, #0x0A
    str     w1, [x19]
    
    // Exit via hypercall
    mov     x0, #1      // exit reason
    hvc     #0
    
guest_mmio_end:
    nop
