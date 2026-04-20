# Fix virtiofs exec on devmount

## Problem

The `incus_devmount` virtiofs mount at `/home/steve/dev` inside VMs `dev-1` and `dev-2` does not allow executing binaries. Any binary compiled into that mount fails with "Bad address (os error 14)". Remounting with `exec` inside the VM has no effect — the restriction is at the virtiofs/incus level.

Current mount inside VM:
```
incus_devmount on /home/steve/dev type virtiofs (rw,relatime)
```

## Diagnose (on the host)

```bash
incus config device show dev-1
incus config device show dev-2
```

Look for the `devmount` disk device. It likely either has no exec-related config (defaulting to noexec) or has an explicit noexec setting.

## Fix

```bash
# For both VMs:
incus config device set dev-1 devmount security.noexec=false
incus config device set dev-2 devmount security.noexec=false

# Restart both VMs for the change to take effect:
incus restart dev-1
incus restart dev-2
```

## Verify (inside the VM after restart)

```bash
# Compile and execute a trivial binary on the mount
echo 'fn main() { println!("exec works"); }' > /home/steve/dev/test_exec.rs
rustc /home/steve/dev/test_exec.rs -o /home/steve/dev/test_exec
/home/steve/dev/test_exec
rm /home/steve/dev/test_exec /home/steve/dev/test_exec.rs
```

If `security.noexec` isn't a recognized config key for the device type, check `incus config device show` output for the exact device type and options — the key name may differ depending on how the disk was added.
