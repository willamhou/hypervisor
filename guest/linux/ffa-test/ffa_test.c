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
#include <linux/scatterlist.h>
#include <linux/mm.h>

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

/*
 * MEM_SHARE E2E test using raw SMC — bypasses the FF-A driver API because
 * pKVM doesn't have the host's RXTX buffers (the driver registered them
 * before pKVM took EL2, so pKVM never intercepted the RXTX_MAP).
 *
 * We allocate our own TX/RX pages, register them via RXTX_MAP (pKVM
 * intercepts and records), construct the FF-A v1.1 MEM_SHARE descriptor
 * manually, and use raw SMC for SHARE/RECLAIM.  DIRECT_REQ still uses
 * the driver API since it doesn't need the TX buffer.
 *
 * FF-A v1.1 descriptor layout (1 page, 1 receiver):
 *   [0..47]  ffa_mem_region header (48 bytes)
 *   [48..63] EMAD — endpoint memory access descriptor (16 bytes for v1.1)
 *   [64..79] composite memory region (16 bytes)
 *   [80..95] constituent address range (16 bytes)
 *   Total = 96 bytes
 */
static void *test_tx_buf;
static void *test_rx_buf;

static int ffa_test_rxtx_setup(void)
{
	struct arm_smccc_res res;

	test_tx_buf = alloc_pages_exact(PAGE_SIZE, GFP_KERNEL);
	test_rx_buf = alloc_pages_exact(PAGE_SIZE, GFP_KERNEL);
	if (!test_tx_buf || !test_rx_buf)
		return -ENOMEM;

	memset(test_tx_buf, 0, PAGE_SIZE);
	memset(test_rx_buf, 0, PAGE_SIZE);

	pr_info("ffa_test: About to call RXTX_MAP tx=0x%llx rx=0x%llx...\n",
		(u64)virt_to_phys(test_tx_buf),
		(u64)virt_to_phys(test_rx_buf));

	arm_smccc_1_1_smc(0xC4000066, /* FFA_FN64_RXTX_MAP — must use 64-bit
			   * variant because pKVM only dispatches FFA_FN64_RXTX_MAP;
			   * the 32-bit 0x84000066 passes through to EL3 unhandled,
			   * leaving pKVM's ffa_buf->tx NULL */
		virt_to_phys(test_tx_buf),
		virt_to_phys(test_rx_buf),
		1, /* 1 page */
		0, 0, 0, 0, &res);

	pr_info("ffa_test: RXTX_MAP returned: a0=0x%llx a2=0x%llx tx_pa=0x%llx rx_pa=0x%llx\n",
		(u64)res.a0, (u64)res.a2,
		(u64)virt_to_phys(test_tx_buf),
		(u64)virt_to_phys(test_rx_buf));

	if (res.a0 != 0x84000061) { /* FFA_SUCCESS_32 */
		pr_err("ffa_test: RXTX_MAP failed\n");
		return -EIO;
	}
	return 0;
}

static void ffa_test_rxtx_teardown(void)
{
	struct arm_smccc_res res;

	arm_smccc_1_1_smc(0x84000067, /* FFA_RXTX_UNMAP */
		0, 0, 0, 0, 0, 0, 0, &res);

	if (test_tx_buf)
		free_pages_exact(test_tx_buf, PAGE_SIZE);
	if (test_rx_buf)
		free_pages_exact(test_rx_buf, PAGE_SIZE);
	test_tx_buf = NULL;
	test_rx_buf = NULL;
}

/*
 * Build FF-A v1.1 MEM_SHARE descriptor in the TX buffer.
 * Returns total descriptor length (96 for 1 page, 1 receiver).
 */
static u32 build_share_desc(void *tx, u16 sender_id, u16 receiver_id,
			    phys_addr_t page_pa)
{
	u8 *buf = tx;

	memset(buf, 0, 96);

	/* ffa_mem_region header (48 bytes) */
	*(u16 *)(buf + 0) = sender_id;		/* sender_id */
	*(u8 *)(buf + 2) = 0;			/* attributes */
	*(u8 *)(buf + 3) = 0;			/* reserved */
	*(u32 *)(buf + 4) = 0;			/* flags: SHARE */
	*(u64 *)(buf + 8) = 0;			/* handle: 0 (assigned by SPMC) */
	*(u64 *)(buf + 16) = 0;		/* tag */
	/* ep_mem_size at offset 24 (u32) — size of each EMAD entry */
	*(u32 *)(buf + 24) = 16;		/* v1.1 EMAD size = 16 */
	/* ep_count at offset 28 (u32) */
	*(u32 *)(buf + 28) = 1;		/* 1 receiver */
	/* ep_mem_offset at offset 32 (u16) — offset to first EMAD */
	*(u16 *)(buf + 32) = 48;		/* EMADs start right after header */
	/* reserved/pad at 34..47 */

	/* EMAD (16 bytes) at offset 48 */
	*(u16 *)(buf + 48) = receiver_id;	/* receiver */
	*(u8 *)(buf + 50) = FFA_MEM_RW;		/* attrs: RW */
	*(u8 *)(buf + 51) = 0;			/* flag */
	*(u32 *)(buf + 52) = 64;		/* composite_off: composite at 64 */

	/* Composite memory region (16 bytes) at offset 64 */
	*(u32 *)(buf + 64) = 1;		/* total_pg_cnt */
	*(u32 *)(buf + 68) = 1;		/* addr_range_cnt */
	*(u64 *)(buf + 72) = 0;		/* reserved */

	/* Constituent (16 bytes) at offset 80 */
	*(u64 *)(buf + 80) = page_pa;		/* address */
	*(u32 *)(buf + 88) = 1;		/* pg_cnt: 1 page */
	*(u32 *)(buf + 92) = 0;		/* reserved */

	return 96;
}

static void ffa_test_mem_share(struct ffa_device *ffa_dev)
{
	const struct ffa_msg_ops *msg_ops;
	struct ffa_send_direct_data data;
	struct arm_smccc_res res;
	void *page_buf;
	phys_addr_t page_pa;
	u64 handle;
	u32 desc_len;
	int ret;

	pr_info("ffa_test: ---- MEM_SHARE E2E Test (SP 0x%04x) ----\n",
		ffa_dev->vm_id);

	msg_ops = ffa_dev->ops->msg_ops;

	/* 1. Allocate a page and zero it */
	page_buf = alloc_pages_exact(PAGE_SIZE, GFP_KERNEL);
	if (!page_buf) {
		pr_err("ffa_test: Failed to allocate page\n");
		return;
	}
	memset(page_buf, 0, PAGE_SIZE);
	page_pa = virt_to_phys(page_buf);
	pr_info("ffa_test: Allocated page: pa=0x%llx\n", (u64)page_pa);

	/* 2. Build MEM_SHARE descriptor in our TX buffer */
	if (!test_tx_buf) {
		pr_err("ffa_test: No TX buffer — RXTX_MAP failed?\n");
		goto cleanup_page;
	}

	desc_len = build_share_desc(test_tx_buf, 0 /* HOST_FFA_ID */,
				    ffa_dev->vm_id, page_pa);
	pr_info("ffa_test: Built descriptor: %u bytes, receiver=0x%04x\n",
		desc_len, ffa_dev->vm_id);

	/* 3. Raw MEM_SHARE_32 SMC */
	arm_smccc_1_1_smc(0x84000073, /* FFA_MEM_SHARE_32 */
		desc_len, /* total_length */
		desc_len, /* fragment_length (no fragmentation) */
		0, 0, 0, 0, 0, &res);

	pr_info("ffa_test: MEM_SHARE: a0=0x%llx a1=0x%llx a2=0x%llx a3=0x%llx\n",
		(u64)res.a0, (u64)res.a1, (u64)res.a2, (u64)res.a3);

	if (res.a0 == 0x84000060) { /* FFA_ERROR */
		pr_err("ffa_test: MEM_SHARE failed: error=%lld\n", (s64)res.a2);
		ffa_test_check("MEM_SHARE returns success", false);
		goto cleanup_page;
	}
	ffa_test_check("MEM_SHARE returns success",
		       res.a0 == 0x84000061 /* FFA_SUCCESS_32 */);

	/* Handle in x2 (lo) and x3 (hi) */
	handle = (res.a2 & 0xFFFFFFFF) | ((res.a3 & 0xFFFFFFFF) << 32);
	ffa_test_check("MEM_SHARE handle != 0", handle != 0);
	pr_info("ffa_test: MEM_SHARE handle=0x%llx\n", handle);

	/* 4. DIRECT_REQ via driver API to trigger SP memory test */
	memset(&data, 0, sizeof(data));
	data.data0 = 0xABCD0001; /* MEM_TEST_MAGIC */
	data.data1 = handle & 0xFFFFFFFF;
	data.data2 = handle >> 32;
	data.data3 = page_pa;

	pr_info("ffa_test: Sending MEM_TEST DIRECT_REQ...\n");
	ret = msg_ops->sync_send_receive(ffa_dev, &data);
	ffa_test_check("MEM_TEST DIRECT_REQ returns success", ret == 0);
	if (ret == 0) {
		u32 expected_x4 = (u32)(handle & 0xFFFFFFFF) + 0x4000;

		ffa_test_check("SP x4 = handle_lo + 0x4000 (mem test proof)",
			       (u32)data.data1 == expected_x4);
		pr_info("ffa_test: SP resp: x4=0x%lx (expect 0x%x)\n",
			data.data1, expected_x4);
	}

	/* 5. Verify page content — SP wrote 0xCAFEFACE */
	{
		u32 val = *(volatile u32 *)page_buf;

		ffa_test_check("Shared page == 0xCAFEFACE (SP wrote it)",
			       val == 0xCAFEFACE);
		pr_info("ffa_test: Page[0] = 0x%08x (expect 0xCAFEFACE)\n",
			val);
	}

	/* 6. Raw MEM_RECLAIM */
	arm_smccc_1_1_smc(0x84000077, /* FFA_MEM_RECLAIM */
		handle & 0xFFFFFFFF,
		handle >> 32,
		0, /* flags */
		0, 0, 0, 0, &res);
	ffa_test_check("MEM_RECLAIM returns success",
		       res.a0 == 0x84000061);
	pr_info("ffa_test: MEM_RECLAIM: a0=0x%llx\n", (u64)res.a0);

cleanup_page:
	free_pages_exact(page_buf, PAGE_SIZE);
	pr_info("ffa_test: ---- MEM_SHARE E2E Test done ----\n");
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

	/* Run MEM_SHARE E2E test for both SP1 and SP2 */
	ffa_test_mem_share(ffa_dev);

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

static void __maybe_unused raw_hvc_diag(void)
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

	/*
	 * pKVM workaround: FF-A driver init runs BEFORE pKVM takes EL2,
	 * so the driver's FFA_VERSION bypasses pKVM and goes directly to
	 * EL3. pKVM's has_version_negotiated flag remains false, causing
	 * all subsequent FF-A calls from our module to fail with -2.
	 * Fix: call FFA_VERSION via SMC to set the flag in pKVM.
	 */
	{
		struct arm_smccc_res ver_res;

		arm_smccc_1_1_smc(0x84000063, 0x00010001, 0, 0, 0, 0, 0, 0,
				  &ver_res);
		pr_info("ffa_test: pKVM VERSION prime: 0x%llx\n",
			(u64)ver_res.a0);
	}

	/*
	 * pKVM workaround #2: register our OWN TX/RX buffers with pKVM.
	 * The host kernel's FF-A driver called RXTX_MAP before pKVM init,
	 * so pKVM never intercepted it and doesn't know about those buffers.
	 * We register our own so raw MEM_SHARE can work.
	 */
	ret = ffa_test_rxtx_setup();
	if (ret)
		pr_warn("ffa_test: RXTX setup failed: %d (MEM_SHARE will skip)\n", ret);

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
	ffa_test_rxtx_teardown();
	ffa_unregister((&ffa_test_driver));
	pr_info("ffa_test: Driver unregistered\n");
}

module_init(ffa_test_init);
module_exit(ffa_test_exit);

MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("FF-A End-to-End Test: DIRECT_REQ + MEM_SHARE (FF-A driver API)");
MODULE_AUTHOR("Hypervisor Project");
