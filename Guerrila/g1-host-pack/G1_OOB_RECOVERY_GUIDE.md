# G1 OOB Recovery Guide

This guide is the recovery path for a host that is reachable through Latitude OOB but is not healthy enough for normal SSH, or no longer accepts the expected installed-OS password.

It is written for the current `G-12` / `109.94.99.15` flow, but the same pattern applies to other hosts that use the same OOB gateway model.

---

## What this guide covers

Use this guide when one or more of the following are true:

- normal SSH to the installed OS is down or unreliable;
- the host boots, but the installed Ubuntu login rejects the expected password;
- management networking was disrupted by AF_XDP, DPDK, VFIO, or driver experiments;
- you need serial-console access before trying another risky NIC attach.

This guide does **not** replace `G1_RUNBOOK.md`; it is the recovery companion for it.

---

## Where the OOB credentials live

Do **not** duplicate live credentials into markdown.

Use these files in `g1-host-pack`:

- `g1-host-pack\oob_credentials` - current OOB gateway host, port, gateway username, console login, and the currently tracked console password for the host
- `g1-host-pack\ssh_file.txt` - OOB gateway private key
- `g1-host-pack\ssh_file.txt.pub` - matching public key

If the installed Ubuntu password is reset during rescue, update `g1-host-pack\oob_credentials` immediately after recovery so the next operator does not have to rediscover it.

---

## Fast decision rule

If OOB gateway auth works but the installed Ubuntu login rejects the tracked OS password:

```text
OOB gateway auth: good
Installed Ubuntu login: bad
=> stop retrying and switch to Rescue Mode
```

Do not burn time retyping the same password repeatedly once it is clear the installed login is wrong.

---

## Part 1 - Log in through the OOB serial gateway

From the local machine, use the OOB gateway key and values stored in `g1-host-pack\oob_credentials`.

Pattern:

```powershell
ssh -i "C:\Shadow Engine\Guerrila\g1-host-pack\ssh_file.txt" -p 2222 <oob_gateway_user>@<oob_gateway_host>
```

For the current host pack, the exact host, port, and gateway user are already recorded in `g1-host-pack\oob_credentials`.

Once connected:

1. Press `Enter` if needed until the serial console login prompt appears.
2. Log in with the console username from `g1-host-pack\oob_credentials`.
3. Use the tracked installed-OS password from `g1-host-pack\oob_credentials`.

Expected banner shape:

```text
[SOL Session operational.  Use ~? for help]
...
G-12 login:
```

If the host key for the OOB gateway changes on the local machine, clear the stale cached key before reconnecting:

```powershell
ssh-keygen -R <oob_gateway_host>
ssh-keygen -R "[<oob_gateway_host>]:<oob_gateway_port>"
```

---

## Part 2 - If the installed OS password does not work, use Rescue Mode

In Latitude:

```text
Server -> Recovery -> Rescue Mode -> Start rescue
```

Use the rescue credentials shown by Latitude for the temporary rescue OS. Those rescue credentials are separate from the OOB gateway key and separate from the installed Ubuntu password.

Once logged into the rescue OS:

```bash
sudo -i
lsblk -f
```

On the current `G-12` host, the installed Ubuntu root partition was:

```text
/dev/nvme1n1p2
```

Do not assume that blindly on other hosts; verify with `lsblk -f`.

Mount the installed root:

```bash
mkdir -p /mnt/host
mount /dev/nvme1n1p2 /mnt/host
```

If the root partition is unclear:

```bash
for p in /dev/nvme*n*p* /dev/sd*; do
  mkdir -p /mnt/test
  mount "$p" /mnt/test 2>/dev/null || continue
  if [ -f /mnt/test/etc/os-release ]; then
    echo "ROOT CANDIDATE: $p"
    cat /mnt/test/etc/os-release
  fi
  umount /mnt/test
done
```

Bind the runtime mounts and chroot:

```bash
mount --bind /dev /mnt/host/dev
mount --bind /proc /mnt/host/proc
mount --bind /sys /mnt/host/sys
mount --bind /run /mnt/host/run

chroot /mnt/host /bin/bash
```

Reset the installed Ubuntu password and preserve sudo:

```bash
passwd ubuntu
usermod -aG sudo ubuntu
passwd -S ubuntu
exit
```

Use a temporary password that is easy to type correctly on a serial console. After resetting it, update `g1-host-pack\oob_credentials` so the tracked console password stays current.

---

## Part 3 - Disable risky leftovers before leaving rescue

Before rebooting back to the installed OS, check for obvious Tier0 / DPDK / XDP leftovers:

```bash
grep -RniE 'xdp|afxdp|dpdk|vfio|shadow-dpdk|shadow-tier0' \
  /mnt/host/etc/systemd/system \
  /mnt/host/etc/cron* \
  /mnt/host/home/ubuntu 2>/dev/null | head -n 200
```

Disable obvious units if they exist:

```bash
chroot /mnt/host /bin/bash -lc "systemctl disable shadow-tier0 2>/dev/null || true"
chroot /mnt/host /bin/bash -lc "systemctl disable stage-dpdk-host 2>/dev/null || true"
```

Drop a safety note for the next session:

```bash
cat > /mnt/host/home/ubuntu/RECOVERY_NOTE_DO_NOT_NATIVE_AFXDP_ENO1.txt <<'EOF'
Do not retry native AF_XDP zero-copy on routed eno1.
Use DPDK diagnostic after VLAN/QinQ fix, or AF_XDP only in safer xdpgeneric/copy/one-queue mode.
Public send remains blocked.
EOF
chown 1000:1000 /mnt/host/home/ubuntu/RECOVERY_NOTE_DO_NOT_NATIVE_AFXDP_ENO1.txt 2>/dev/null || true
```

Unmount cleanly and reboot:

```bash
umount -R /mnt/host
reboot
```

---

## Part 4 - First login after recovery

After reboot, prefer the OOB serial console first. Confirm the installed OS login works before trusting normal SSH.

Once back in the installed OS:

```bash
sudo -i
ip link set dev eno1 xdp off 2>/dev/null || true
rm -f /sys/fs/bpf/shadow_xsks_map 2>/dev/null || true
modprobe ixgbe
dpdk-devbind.py --status 2>/dev/null || true
ip -br addr
ip route
systemctl restart ssh
```

If `shadow-tier0` or DPDK helper services exist on that host, keep them disabled until the host is re-qualified.

---

## Current host-specific notes for `109.94.99.15`

- `eno1` is the management/default-route NIC.
- Do **not** churn native AF_XDP zero-copy on routed `eno1` as a routine step.
- The disruptive part on `.15` is the **native attach/detach transition** on `ixgbe`: carrier can drop, PHC / queue layout can reinitialize, and normal SSH may disappear until the link and `systemd-networkd` settle again.
- If AF_XDP must touch management `eno1`, require OOB coverage for the first native arm. Once native XDP is attached and stable, `.15` has now been proven to keep `eno1` routable and accept normal SSH while `afxdp_bind_mode: zero_copy` remains live.
- The guarded native Run38 path is now `g1-host-pack\scripts\38_any_hunt_publicrpc_blackhole_native_wait.sh`.
- That waiter will only launch native zero-copy Run38 after both conditions are true:
  - local-Agave preflight is green
  - explicit OOB arming exists at `/home/ubuntu/shadow-host-audit/dpdk-runs/run38_native_oob_ready.flag`
- The current DPDK issue on `.15` is not "RX-dark NIC"; the current gap is between DPDK RX and the in-process classifier/feed path.
- Public send remains blocked; recovery does not change routing policy or rollout gates.

---

## What to update after a successful recovery

After the host is usable again, update:

1. `g1-host-pack\oob_credentials` if the installed Ubuntu password changed
2. `g1-host-pack\G1_RUNBOOK.md` if the failure exposed a new repeatable pitfall
3. the current handoff / QC docs if the recovery changed host disposition

At minimum, preserve:

- the fact that OOB login worked or failed
- whether rescue mode was required
- the installed root partition used
- whether the password was reset
- whether XDP / DPDK leftovers had to be cleared
- the final `ip -br addr` and `ip route` state

