"""B13 spike perf: `herdr desk` with N windows all running `yes`, fixed 200x60 host.

Reports desk CPU (incl. reaped session children), server CPU delta, frames and draw timing.
usage: python3 desk_perf.py <herdr-binary> <seconds> <N...>   (env selects the session)
"""
import fcntl, json, os, pty, resource, select, struct, subprocess, sys, termios, time

H, SECS, COUNTS = sys.argv[1], float(sys.argv[2]), [int(n) for n in sys.argv[3:]]


def cli(*args):
    out = subprocess.run([H, *args], capture_output=True, text=True, check=True).stdout
    return json.loads(out)["result"]


def server_cpu(pid):
    out = subprocess.run(["ps", "-o", "time=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    m, s = out.split(":")
    return int(m) * 60 + float(s)


def desk_panes():
    for w in cli("workspace", "list")["workspaces"]:
        if w["label"] == "desk":
            return w["workspace_id"], cli("pane", "list", "--workspace", w["workspace_id"])["panes"]
    created = cli("workspace", "create", "--label", "desk", "--no-focus")
    return created["workspace"]["workspace_id"], [created["root_pane"]]


def run_desk(stats_path):
    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(H, [H, "desk"], dict(os.environ, TERM="xterm-256color", HERDR_DESK_STATS=stats_path))
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 60, 200, 0, 0))
    host_bytes = 0
    end = time.time() + SECS
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                host_bytes += len(os.read(fd, 1 << 16))
            except OSError:
                break
    os.write(fd, b"\x1cq")
    deadline = time.time() + 5
    while time.time() < deadline:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                os.read(fd, 1 << 16)
            except OSError:
                pass
        done, status, usage = os.wait4(pid, os.WNOHANG)
        if done:
            return usage, host_bytes
    raise SystemExit("desk did not exit")


server_pid = int(sys.argv[0] and os.environ["DESK_SERVER_PID"])
for n in COUNTS:
    ws, panes = desk_panes()
    while len(panes) < n:
        cli("tab", "create", "--workspace", ws, "--no-focus")
        panes = cli("pane", "list", "--workspace", ws)["panes"]
    for extra in panes[n:]:
        subprocess.run([H, "pane", "close", extra["pane_id"]], capture_output=True)
    panes = cli("pane", "list", "--workspace", ws)["panes"]
    assert len(panes) == n, len(panes)
    for p in panes:
        subprocess.run([H, "pane", "send-keys", p["pane_id"], "C-c"], capture_output=True)
        subprocess.run([H, "pane", "run", p["pane_id"], "seq 1 1000000000"], capture_output=True, check=True)
    time.sleep(1)
    idle_before = server_cpu(server_pid)
    time.sleep(SECS)
    server_no_desk = 100 * (server_cpu(server_pid) - idle_before) / SECS
    before = server_cpu(server_pid)
    stats_path = f"/var/tmp/desk-stats-{n}.json"
    usage, host_bytes = run_desk(stats_path)
    after = server_cpu(server_pid)
    stats = json.load(open(stats_path))
    print(json.dumps({
        "windows": n,
        "seconds": SECS,
        "desk_cpu_pct": round(100 * (usage.ru_utime + usage.ru_stime) / SECS, 1),
        "server_cpu_pct_no_desk": round(server_no_desk, 1),
        "server_cpu_pct_with_desk": round(100 * (after - before) / SECS, 1),
        "frames_per_s": round(stats["frames"] / SECS),
        "frame_kib_per_s": round(stats["frame_bytes"] / 1024 / SECS),
        "draws_per_s": round(stats["draws"] / SECS),
        "draw_avg_ms": stats["draw_avg_us"] / 1000,
        "draw_max_ms": stats["draw_max_us"] / 1000,
        "host_kib_per_s": round(host_bytes / 1024 / SECS),
    }))
