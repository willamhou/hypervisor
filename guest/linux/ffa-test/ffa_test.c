// SPDX-License-Identifier: GPL-2.0
/*
 * FF-A DIRECT_REQ Test — Linux FF-A Client Driver
 *
 * Registers as an FF-A driver that matches SP1 (0x8001) and SP2 (0x8002)
 * by UUID. On probe(), sends DIRECT_REQ via the FF-A transport layer
 * (ffa_dev->ops->msg_ops->sync_send_receive) and validates responses.
 *
 * This is the correct approach for pKVM: raw SMC calls from EL1 bypass
 * pKVM's FF-A proxy expectations and may hang. The FF-A driver API goes
 * through the proper kernel→pKVM→SPMD→SPMC chain.
 *
 * Usage: insmod ffa_test.ko
 * Results printed to kernel log (dmesg).
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/arm_ffa.h>
#include <linux/arm-smccc.h>

static int tests_run;
static int tests_pass;

static void ffa_test_check(const char *name, bool cond)
{
	tests_run++;
	if (cond) {
		tests_pass++;
		pr_info("ffa_test: [PASS] %s\n", name);
	} else {
		pr_err("ffa_test: [FAIL] %s\n", name);
	}
}

static int ffa_test_probe(struct ffa_device *ffa_dev)
{
	const struct ffa_msg_ops *msg_ops;
	struct ffa_send_direct_data data;
	int sp_id;
	int ret;
	char desc[80];

	sp_id = ffa_dev->vm_id;
	pr_info("ffa_test: ---- Probing SP 0x%04x ----\n", sp_id);

	if (!ffa_dev->ops || !ffa_dev->ops->msg_ops) {
		pr_err("ffa_test: No ops for SP 0x%04x\n", sp_id);
		return -ENODEV;
	}

	msg_ops = ffa_dev->ops->msg_ops;
	if (!msg_ops->sync_send_receive) {
		pr_err("ffa_test: No sync_send_receive for SP 0x%04x\n", sp_id);
		return -ENODEV;
	}

	/* Check partition supports direct receive */
	if (!ffa_partition_supports_direct_recv(ffa_dev)) {
		pr_warn("ffa_test: SP 0x%04x does not advertise DIRECT_RECV\n",
			sp_id);
	}

	/* Send DIRECT_REQ with echo test payload:
	 *   data0 (x3) = 0xAAAA — echoed back
	 *   data1 (x4) = 0xBBBB — SP adds 0x1000 → expect 0xCBBB
	 *   data2 (x5) = 0xCCCC — echoed back
	 *   data3 (x6) = 0xDDDD — echoed back
	 *   data4 (x7) = 0xEEEE — echoed back
	 */
	memset(&data, 0, sizeof(data));
	data.data0 = 0xAAAA;
	data.data1 = 0xBBBB;
	data.data2 = 0xCCCC;
	data.data3 = 0xDDDD;
	data.data4 = 0xEEEE;

	pr_info("ffa_test: Sending DIRECT_REQ to SP 0x%04x...\n", sp_id);
	ret = msg_ops->sync_send_receive(ffa_dev, &data);
	pr_info("ffa_test: DIRECT_REQ to SP 0x%04x: ret=%d\n", sp_id, ret);
	pr_info("ffa_test:   x3=0x%lx x4=0x%lx x5=0x%lx x6=0x%lx x7=0x%lx\n",
		data.data0, data.data1, data.data2, data.data3, data.data4);

	snprintf(desc, sizeof(desc),
		 "DIRECT_REQ to SP 0x%04x returns success", sp_id);
	ffa_test_check(desc, ret == 0);

	if (ret == 0) {
		snprintf(desc, sizeof(desc),
			 "SP 0x%04x x3 echoes 0xAAAA", sp_id);
		ffa_test_check(desc, data.data0 == 0xAAAA);

		snprintf(desc, sizeof(desc),
			 "SP 0x%04x x4 = 0xBBBB + 0x1000", sp_id);
		ffa_test_check(desc, data.data1 == 0xCBBB);

		snprintf(desc, sizeof(desc),
			 "SP 0x%04x x5 echoes 0xCCCC", sp_id);
		ffa_test_check(desc, data.data2 == 0xCCCC);
	}

	pr_info("ffa_test: ---- SP 0x%04x done (%d/%d) ----\n",
		sp_id, tests_pass, tests_run);

	/* Return 0 to bind — we'll unload via rmmod to print final results */
	return 0;
}

static void ffa_test_remove(struct ffa_device *ffa_dev)
{
	pr_info("ffa_test: Removing SP 0x%04x binding\n", ffa_dev->vm_id);
}

/*
 * UUID table — must match what the FF-A bus discovers via PARTITION_INFO_GET.
 * These are the byte-swapped UUIDs as seen in sysfs:
 *   SP1: 78563412-7856-3412-7856-341278563412
 *   SP2: ddccbbaa-ddcc-bbaa-ddcc-bbaaddccbbaa
 */
static const struct ffa_device_id ffa_test_ids[] = {
	/* SP1 (0x8001) */
	{ UUID_INIT(0x78563412, 0x7856, 0x3412,
		    0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12) },
	/* SP2 (0x8002) */
	{ UUID_INIT(0xddccbbaa, 0xddcc, 0xbbaa,
		    0xdd, 0xcc, 0xbb, 0xaa, 0xdd, 0xcc, 0xbb, 0xaa) },
	{}
};

static struct ffa_driver ffa_test_driver = {
	.name = "ffa_test",
	.probe = ffa_test_probe,
	.remove = ffa_test_remove,
	.id_table = ffa_test_ids,
};

static void raw_hvc_diag(void)
{
	struct arm_smccc_res res;

	/* Test 1: Raw FFA_VERSION — should return version 1.1
	 * Under pKVM, guest FF-A calls MUST use HVC (not SMC).
	 * pKVM's kvm_handle_pvm_hvc64 routes FF-A to kvm_guest_ffa_handler.
	 * SMC falls through to host kernel and hangs on DIRECT_REQ.
	 */
	arm_smccc_1_1_hvc(0x84000063, 0x00010001, 0, 0, 0, 0, 0, 0, &res);
	pr_info("ffa_test: [RAW] FFA_VERSION: x0=0x%lx\n", res.a0);

	/* Test 2: Raw FFA_ID_GET — should return our VM's FFA ID */
	arm_smccc_1_1_hvc(0x84000069, 0, 0, 0, 0, 0, 0, 0, &res);
	pr_info("ffa_test: [RAW] FFA_ID_GET: x0=0x%lx x2=0x%lx\n",
		res.a0, res.a2);

	/* Test 3: Raw HVC DIRECT_REQ to SP 0x8001.
	 * Under pKVM, this goes through kvm_handle_pvm_hvc64 →
	 * kvm_guest_ffa_handler → do_ffa_direct_msg → nvhe_arm_smccc_1_2_smc.
	 * x1 = src(vm_ffa_id) | dst(0x8001), x3=0xaaaa, x4=0xbbbb
	 *
	 * NOTE: The source ID in x1[31:16] MUST match our VM's FFA handle
	 * (returned by FFA_ID_GET in x2). pKVM validates this.
	 */
	{
		u16 our_id = (u16)res.a2;  /* from FFA_ID_GET above */
		u32 endpoints = ((u32)our_id << 16) | 0x8001;

		pr_info("ffa_test: [RAW] Our FFA ID = 0x%04x\n", our_id);
		pr_warn("ffa_test: [RAW] About to send raw DIRECT_REQ to SP 0x8001...\n");
		arm_smccc_1_1_hvc(0x8400006f, endpoints, 0, 0xaaaa, 0xbbbb,
				  0xcccc, 0xdddd, 0xeeee, &res);
		pr_warn("ffa_test: [RAW] DIRECT_REQ result: x0=0x%lx x1=0x%lx x2=0x%lx x3=0x%lx\n",
			res.a0, res.a1, res.a2, res.a3);
	}
}

static int __init ffa_test_init(void)
{
	int ret;

	pr_info("ffa_test: ============================================\n");
	pr_info("ffa_test:   FF-A DIRECT_REQ End-to-End Test\n");
	pr_info("ffa_test: ============================================\n");

	/* Run raw diagnostics first */
	pr_info("ffa_test: --- Raw diagnostics ---\n");
	raw_hvc_diag();

	/* Bypass test: send DIRECT_REQ with x3=0xDEAD0001.
	 * SPMC returns fake DIRECT_RESP without entering the SP.
	 * If this works but real DIRECT_REQ hangs, the issue is in SP entry/exit.
	 * If this also hangs, the issue is in the DIRECT_RESP world switch itself.
	 */
	{
		struct arm_smccc_res smc_res;
		u32 ep = (0u << 16) | 0x8001;

		/* Must negotiate FFA version via SMC first — pKVM's host FFA handler
		 * blocks all non-VERSION SMC calls until has_version_negotiated is set.
		 * This is per-CPU, so must run on the same CPU as DIRECT_REQ.
		 */
		arm_smccc_1_1_smc(0x84000063, 0x00010001, 0, 0, 0, 0, 0, 0, &smc_res);
		pr_info("ffa_test: [SMC] FFA_VERSION: x0=0x%lx\n", smc_res.a0);

		pr_warn("ffa_test: [BYPASS] Sending DIRECT_REQ with x3=0xDEAD0001 (no SP entry)...\n");
		arm_smccc_1_1_smc(0x8400006f, ep, 0, 0xDEAD0001, 0, 0, 0, 0, &smc_res);
		pr_info("ffa_test: [BYPASS] Result: x0=0x%lx x1=0x%lx x2=0x%lx x3=0x%lx\n",
			smc_res.a0, smc_res.a1, smc_res.a2, smc_res.a3);

		if (smc_res.a0 == 0x84000070) {
			pr_info("ffa_test: [BYPASS] SUCCESS — world switch works without SP entry\n");

			/* Skip MINI test (DEAD0002) — it corrupts NWd EL1 state because
			 * it enters SP without saving/restoring EL1 registers.
			 * Go straight to REAL DIRECT_REQ which uses full dispatch_to_sp().
			 */
			pr_warn("ffa_test: [REAL] Now testing FULL DIRECT_REQ to SP 0x8001...\n");
			arm_smccc_1_1_smc(0x8400006f, ep, 0, 0xaaaa, 0xbbbb,
					  0xcccc, 0xdddd, 0xeeee, &smc_res);
			pr_info("ffa_test: [REAL] Result: x0=0x%lx x1=0x%lx x3=0x%lx\n",
				smc_res.a0, smc_res.a1, smc_res.a3);
		} else {
			pr_err("ffa_test: [BYPASS] FAILED — x0=0x%lx (even bypass hangs/fails)\n",
			       smc_res.a0);
		}
	}
	pr_info("ffa_test: --- End raw diagnostics ---\n");

	tests_run = 0;
	tests_pass = 0;

	ret = ffa_register((&ffa_test_driver));
	if (ret) {
		pr_err("ffa_test: ffa_register failed: %d\n", ret);
		pr_info("ffa_test: Results: 0/0 (driver registration failed)\n");
		return ret;
	}

	pr_info("ffa_test: Driver registered — probe called per matched SP\n");
	pr_info("ffa_test: ============================================\n");
	pr_info("ffa_test:   Results: %d/%d PASS\n", tests_pass, tests_run);
	pr_info("ffa_test: ============================================\n");

	return 0;
}

static void __exit ffa_test_exit(void)
{
	ffa_unregister((&ffa_test_driver));
	pr_info("ffa_test: Driver unregistered\n");
}

module_init(ffa_test_init);
module_exit(ffa_test_exit);

MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("FF-A DIRECT_REQ End-to-End Test (FF-A driver API)");
MODULE_AUTHOR("Hypervisor Project");
