//! Unit tests for SpContext — SP state machine and context management.

use hypervisor::sp_context::{SpContext, SpState};

pub fn run_tests() {
    hypervisor::log_info!("  test_sp_context...\n");
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

    // Test 22-23: owns_intid returns true for assigned INTIDs
    let mut ctx6 = SpContext::new(0x8002, 0x0e400000, 0x0e500000, [0; 4]);
    ctx6.set_owned_intids([29, 0, 0, 0]);
    assert!(ctx6.owns_intid(29));
    assert!(!ctx6.owns_intid(27));
    pass += 2;

    // Test 24: owns_intid returns false for unset (zero) slots
    assert!(!ctx6.owns_intid(0));
    pass += 1;

    // Test 25-27: set_pending_irq / take_pending_irq lifecycle
    assert!(!ctx6.has_pending_irq());
    ctx6.set_pending_irq(29);
    assert!(ctx6.has_pending_irq());
    assert_eq!(ctx6.take_pending_irq(), Some(29));
    pass += 3;

    // Test 28: take_pending_irq returns None after take
    assert_eq!(ctx6.take_pending_irq(), None);
    pass += 1;

    // Test 29-33: pending IRQ queue keeps multiple distinct INTIDs
    ctx6.set_pending_irq(29);
    ctx6.set_pending_irq(30);
    ctx6.set_pending_irq(31);
    assert_eq!(ctx6.take_pending_irq(), Some(29));
    assert_eq!(ctx6.take_pending_irq(), Some(30));
    assert_eq!(ctx6.take_pending_irq(), Some(31));
    assert_eq!(ctx6.take_pending_irq(), None);
    pass += 5;

    // Test 34-35: duplicate pending IRQ is deduplicated
    ctx6.set_pending_irq(45);
    ctx6.set_pending_irq(45);
    assert_eq!(ctx6.take_pending_irq(), Some(45));
    assert_eq!(ctx6.take_pending_irq(), None);
    pass += 2;

    // Test 36-38: preempted CPU ownership tracking
    assert_eq!(ctx6.preempted_cpu(), None);
    ctx6.set_preempted_cpu(2);
    assert_eq!(ctx6.preempted_cpu(), Some(2));
    ctx6.clear_preempted_cpu();
    assert_eq!(ctx6.preempted_cpu(), None);
    pass += 3;

    // Test 39-42: owner CPU claim/migrate/clear lifecycle
    assert_eq!(ctx6.owner_cpu(), None);
    assert!(ctx6.try_claim_owner_cpu(1));
    assert_eq!(ctx6.owner_cpu(), Some(1));
    assert!(ctx6.try_migrate_owner_cpu(1, 3));
    assert_eq!(ctx6.owner_cpu(), Some(3));
    ctx6.clear_owner_cpu();
    assert_eq!(ctx6.owner_cpu(), None);
    pass += 4;

    // ── G1: Complete illegal state transition matrix (7 remaining) ──

    // Test 43-44: Idle → Preempted / Blocked are invalid
    let mut ctx_g1 = SpContext::new(0x9001, 0x0e300000, 0x0e400000, [0; 4]);
    ctx_g1.transition_to(SpState::Idle).unwrap();
    assert!(ctx_g1.transition_to(SpState::Preempted).is_err());
    assert!(ctx_g1.transition_to(SpState::Blocked).is_err());
    pass += 2;

    // Test 45-46: Reset → Preempted / Blocked are invalid
    let mut ctx_g1b = SpContext::new(0x9002, 0x0e300000, 0x0e400000, [0; 4]);
    assert!(ctx_g1b.transition_to(SpState::Preempted).is_err());
    assert!(ctx_g1b.transition_to(SpState::Blocked).is_err());
    pass += 2;

    // Test 47-48: Blocked → Idle / Preempted are invalid
    let mut ctx_g1c = SpContext::new(0x9003, 0x0e300000, 0x0e400000, [0; 4]);
    ctx_g1c.transition_to(SpState::Idle).unwrap();
    ctx_g1c.transition_to(SpState::Running).unwrap();
    ctx_g1c.transition_to(SpState::Blocked).unwrap();
    assert!(ctx_g1c.transition_to(SpState::Idle).is_err());
    assert!(ctx_g1c.transition_to(SpState::Preempted).is_err());
    pass += 2;

    // Test 49: Preempted → Blocked is invalid
    let mut ctx_g1d = SpContext::new(0x9004, 0x0e300000, 0x0e400000, [0; 4]);
    ctx_g1d.transition_to(SpState::Idle).unwrap();
    ctx_g1d.transition_to(SpState::Running).unwrap();
    ctx_g1d.transition_to(SpState::Preempted).unwrap();
    assert!(ctx_g1d.transition_to(SpState::Blocked).is_err());
    pass += 1;

    // ── G2: CAS (try_transition) failure semantics ──

    // Test 50: SP in Idle, CAS expect Running → fail with Err(Idle)
    let mut ctx_g2 = SpContext::new(0x9005, 0x0e300000, 0x0e400000, [0; 4]);
    ctx_g2.transition_to(SpState::Idle).unwrap();
    let result = ctx_g2.try_transition(SpState::Running, SpState::Preempted);
    assert_eq!(result, Err(SpState::Idle));
    pass += 1;

    // Test 51: SP in Running, CAS expect Idle → fail with Err(Running)
    ctx_g2.transition_to(SpState::Running).unwrap();
    let result = ctx_g2.try_transition(SpState::Idle, SpState::Running);
    assert_eq!(result, Err(SpState::Running));
    pass += 1;

    // Test 52: CAS success path — Running → Preempted
    let result = ctx_g2.try_transition(SpState::Running, SpState::Preempted);
    assert_eq!(result, Ok(()));
    assert_eq!(ctx_g2.state(), SpState::Preempted);
    pass += 2;

    // ── G3: IRQ queue overflow ──

    // Test 54-58: Fill 4 slots, 5th is dropped, drain returns 4 then None
    let ctx_g3 = SpContext::new(0x9006, 0x0e300000, 0x0e400000, [0; 4]);
    ctx_g3.set_pending_irq(100);
    ctx_g3.set_pending_irq(101);
    ctx_g3.set_pending_irq(102);
    ctx_g3.set_pending_irq(103);
    ctx_g3.set_pending_irq(104); // 5th — should be dropped (log warning)
    assert_eq!(ctx_g3.take_pending_irq(), Some(100));
    assert_eq!(ctx_g3.take_pending_irq(), Some(101));
    assert_eq!(ctx_g3.take_pending_irq(), Some(102));
    assert_eq!(ctx_g3.take_pending_irq(), Some(103));
    assert_eq!(ctx_g3.take_pending_irq(), None); // 104 was dropped
    pass += 5;

    hypervisor::log_info!("    {} assertions passed\n", pass);
}
