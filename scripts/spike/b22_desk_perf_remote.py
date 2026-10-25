"""B22 spike perf: `herdr desk --ssh` with N remote windows all running `seq`, fixed 200x60 host.

Reports desk CPU (incl. reaped ssh children), ssh master CPU, remote server CPU, frames, draw timing,
wire bytes on the route interface, and TCP connections / ssh processes to the target mid-run.
usage: python3 desk_perf_remote.py <herdr-binary> <target> <remote-session> <seconds> <N...>
"""
import fcntl, json, os, pty, re, select, socket, struct, subprocess, sys, termios, time

H, TARGET, SESSION, SECS = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4])
COUNTS = [int(n) for n in sys.argv[5:]]
IP = socket.gethostbyname(TARGET)
IFACE = re.search(r"interface: (\S+)", subprocess.run(["route", "-n", "get", IP], capture_output=True, text=True).stdout).group(1)


def rsh(cmd):
    return subprocess.run(["ssh", "-o", "BatchMode=yes", TARGET, "--", cmd], capture_output=True, text=True, check=True).stdout


def cli(*args):
    return json.loads(rsh(f"HERDR_SESSION={SESSION} herdr " + " ".join(f"'{a}'" for a in args)))["result"]


def remote_server_cpu():
    # Seconds of CPU of the herdr server whose environment names this session.
    out = rsh(
        "for p in $(pgrep -f 'herdr server'); do "
        f"tr '\\0' '\\n' < /proc/$p/environ 2>/dev/null | grep -qx HERDR_SESSION={SESSION} && "
        "awk '{print ($14+$15)}' /proc/$p/stat; done; getconf CLK_TCK"
    ).split()
    return float(out[0]) / float(out[-1])


def local_cpu(pid):
    out = subprocess.run(["ps", "-o", "time=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    if not out:
        return 0.0
    m, s = out.split(":")
    return int(m) * 60 + float(s)


def wire_bytes():
    out = subprocess.run(["netstat", "-ibn", "-I", IFACE], capture_output=True, text=True).stdout.splitlines()
    row = out[1].split()  # link row has no Address column: index from the right
    return int(row[-5]) + int(row[-2])


def proc_bytes(pid):
    out = subprocess.run(["nettop", "-P", "-L", "1", "-J", "bytes_in,bytes_out", "-p", str(pid)], capture_output=True, text=True).stdout
    for line in out.splitlines():
        if line.startswith("ssh."):
            _, i, o, *_ = line.split(",")
            return int(i) + int(o)
    return 0


def ssh_master_pid():
    out = subprocess.run(["pgrep", "-f", r"ssh: /tmp/hssh-.*\[mux\]"], capture_output=True, text=True).stdout.split()
    return [int(p) for p in out]


def connections():
    out = subprocess.run(["lsof", "-nP", f"-iTCP@{IP}", "-sTCP:ESTABLISHED"], capture_output=True, text=True).stdout
    rows = [l for l in out.splitlines()[1:] if l.strip()]
    ssh_procs = subprocess.run(["pgrep", "-fx", f"ssh -T .* {TARGET} -- .*"], capture_output=True, text=True).stdout.split()
    return len(rows), [" ".join(r.split()[:2]) for r in rows], len(ssh_procs)


def run_desk(stats_path):
    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(H, [H, "desk", "--ssh", TARGET, "--remote-session", SESSION],
                   dict(os.environ, TERM="xterm-256color", HERDR_DESK_STATS=stats_path))
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 60, 200, 0, 0))
    end = time.time() + SECS
    mid = None
    while time.time() < end:
        if mid is None and time.time() > end - SECS / 2:
            mid = connections()
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                os.read(fd, 1 << 16)
            except OSError:
                break
    os.write(fd, b"\x1cq")
    deadline = time.time() + 8
    while time.time() < deadline:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                os.read(fd, 1 << 16)
            except OSError:
                pass
        done, status, usage = os.wait4(pid, os.WNOHANG)
        if done:
            return usage, mid
    raise SystemExit("desk did not exit")


for n in COUNTS:
    tabs = cli("tab", "list")["tabs"]
    ws = tabs[0]["workspace_id"]
    while len(tabs) < n:
        cli("tab", "create", "--workspace", ws, "--no-focus")
        tabs = cli("tab", "list")["tabs"]
    for extra in tabs[n:]:
        cli("tab", "close", extra["tab_id"])
    panes = cli("pane", "list")["panes"]
    for p in panes:
        rsh(f"HERDR_SESSION={SESSION} herdr pane send-keys {p['pane_id']} C-c; "
            f"HERDR_SESSION={SESSION} herdr pane run {p['pane_id']} 'seq 1 1000000000' >/dev/null")
    time.sleep(1)
    before = remote_server_cpu()
    time.sleep(SECS)
    server_no_desk = 100 * (remote_server_cpu() - before) / SECS
    masters = ssh_master_pid()
    master_before = sum(local_cpu(p) for p in masters)
    wire_before = wire_bytes()
    master_bytes_before = sum(proc_bytes(p) for p in masters)
    before = remote_server_cpu()
    stats_path = f"/var/tmp/desk-b22-stats-{n}.json"
    usage, mid = run_desk(stats_path)
    server_with = 100 * (remote_server_cpu() - before) / SECS
    wire = wire_bytes() - wire_before
    master_bytes = sum(proc_bytes(p) for p in masters) - master_bytes_before
    masters_after = ssh_master_pid()
    master_cpu = sum(local_cpu(p) for p in masters_after) - master_before
    stats = json.load(open(stats_path))
    print(json.dumps({
        "windows": n,
        "panes": len(panes),
        "seconds": SECS,
        "desk_cpu_pct": round(100 * (usage.ru_utime + usage.ru_stime) / SECS, 1),
        "ssh_master_cpu_pct": round(100 * master_cpu / SECS, 1),
        "ssh_masters": masters_after,
        "remote_server_cpu_pct_no_desk": round(server_no_desk, 1),
        "remote_server_cpu_pct_with_desk": round(server_with, 1),
        "frames_per_s": round(stats["frames"] / SECS),
        "frame_kib_per_s": round(stats["frame_bytes"] / 1024 / SECS),
        "wire_kib_per_s": round(wire / 1024 / SECS),
        "master_wire_kib_per_s": round(master_bytes / 1024 / SECS, 1),
        "draw_avg_ms": stats["draw_avg_us"] / 1000,
        "tcp_to_target_mid_run": mid[0],
        "tcp_owners": mid[1],
        "ssh_clients_mid_run": mid[2],
    }), flush=True)
