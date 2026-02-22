//! Unit tests for SpContext — SP state machine and context management.

use hypervisor::sp_context::{SpContext, SpState};

pub fn run_tests() {
    crate::uart_puts(b"  test_sp_context...\n");
    let mut pass = 0u32;

    // Test 1-3: New SpContext has correct initial state
    let ctx = SpContext::new(0x8001, 0x0e300000, 0x0e400000, [0; 4]);
    assert_eq!(ctx.state(), SpState::Reset);
    assert_eq!(ctx.sp_id(), 0x8001);
    assert_eq!(ctx.entry_point(), 0x0e300000);
    pass += 3;

    // Test 4-5: State transitions Reset -> Idle
    let mut ctx = SpContext::new(0x8001, 0x0e300000, 0x0e400000, [0; 4]);
    assert!(ctx.transition_to(SpState::Idle).is_ok());
    assert_eq!(ctx.state(), SpState::Idle);
    pass += 2;

    // Test 6-7: State transitions Idle -> Running
    assert!(ctx.transition_to(SpState::Running).is_ok());
    assert_eq!(ctx.state(), SpState::Running);
    pass += 2;

    // Test 8-9: State transitions Running -> Idle
    assert!(ctx.transition_to(SpState::Idle).is_ok());
    assert_eq!(ctx.state(), SpState::Idle);
    pass += 2;

    // Test 10: Invalid transition Reset -> Running
    let mut ctx2 = SpContext::new(0x8002, 0x0e400000, 0x0e500000, [0; 4]);
    assert!(ctx2.transition_to(SpState::Running).is_err());
    pass += 1;

    // Test 11-13: VcpuContext fields
    let ctx3 = SpContext::new(0x8001, 0x0e300000, 0x0e400000, [0; 4]);
    assert_eq!(ctx3.vcpu_ctx().pc, 0x0e300000);
    assert_eq!(ctx3.vcpu_ctx().sp, 0x0e400000);
    assert_eq!(ctx3.vcpu_ctx().spsr_el2, 0x3C5);
    pass += 3;

    // Test 14-16: set_args and get_args
    let mut ctx4 = SpContext::new(0x8001, 0x0e300000, 0x0e400000, [0; 4]);
    ctx4.set_args(0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22);
    let (x0, _x1, _x2, x3, _x4, _x5, _x6, x7) = ctx4.get_args();
    assert_eq!(x0, 0xAA);
    assert_eq!(x3, 0xDD);
    assert_eq!(x7, 0x22);
    pass += 3;

    // Test 17-18: Running -> Preempted transition
    let mut ctx5 = SpContext::new(0x8003, 0x0e300000, 0x0e400000, [0; 4]);
    ctx5.transition_to(SpState::Idle).unwrap();
    ctx5.transition_to(SpState::Running).unwrap();
    assert!(ctx5.transition_to(SpState::Preempted).is_ok());
    assert_eq!(ctx5.state(), SpState::Preempted);
    pass += 2;

    // Test 19-20: Preempted -> Running transition (resume via FFA_RUN)
    assert!(ctx5.transition_to(SpState::Running).is_ok());
    assert_eq!(ctx5.state(), SpState::Running);
    pass += 2;

    // Test 21: Preempted -> Idle is invalid (must go through Running first)
    ctx5.transition_to(SpState::Preempted).unwrap();
    assert!(ctx5.transition_to(SpState::Idle).is_err());
    pass += 1;

    crate::uart_puts(b"    ");
    crate::print_u32(pass);
    crate::uart_puts(b" assertions passed\n");
}
