/*
 * Simple Guest Test Program
 * 
 * This is a minimal guest program to test the vCPU framework.
 * It will:
 * 1. Print a character via hypercall
 * 2. Execute a WFI instruction (will trap to hypervisor)
 * 3. Exit via hypercall
 */

.section .text
.global guest_entry

guest_entry:
    // Print 'G' (Guest started)
    mov     x0, #0          // Hypercall 0: print character
    mov     x1, #'G'        // Character to print
    hvc     #0              // Invoke hypercall
    
    // Print '!'
    mov     x0, #0
    mov     x1, #'!'
    hvc     #0
    
    // Print newline
    mov     x0, #0
    mov     x1, #'\n'
    hvc     #0
    
    // Execute WFI to test trap handling
    wfi
    
    // Exit guest
    mov     x0, #1          // Hypercall 1: exit guest
    hvc     #0
    
    // Should not reach here
halt:
    wfe
    b       halt
