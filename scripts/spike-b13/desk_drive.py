"""Drive `herdr desk` in a pty (B13 spike): send keys / SGR mouse, print the emulated screen.

usage: uv run --with pyte --with wcwidth python3 desk_drive.py <scenario.json>
scenario: {"cols":120,"rows":36,"env":{...},"args":[...],"steps":[[...], ...]}
steps: ["shot", label] | ["key", text] | ["mouse", cb, x, y, up] | ["drag", x0, y0, x1, y1]
       | ["sleep", seconds] | ["resize", cols, rows] | ["wait_exit", seconds]
"""
import fcntl, json, os, pty, select, signal, struct, sys, termios, time

import pyte
from wcwidth import wcwidth


def show(screen):
    rows = []
    for y in range(screen.lines):
        line, skip = [], False
        for x in range(screen.columns):
            ch = screen.buffer[y][x].data
            if skip:
                skip = False
                continue
            if not ch:
                line.append("?")
                continue
            line.append(ch)
            skip = wcwidth(ch[0]) == 2
        rows.append("".join(line).rstrip())
    return rows


def main():
    spec = json.load(open(sys.argv[1]))
    cols, rows = spec.get("cols", 120), spec.get("rows", 36)
    pid, fd = pty.fork()
    if pid == 0:
        env = dict(os.environ, TERM="xterm-256color", **spec.get("env", {}))
        os.execvpe(spec["args"][0], spec["args"], env)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    screen = pyte.Screen(cols, rows)
    stream = pyte.ByteStream(screen)

    def pump(t=0.4):
        end = time.time() + t
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.05)
            if r:
                try:
                    stream.feed(os.read(fd, 65536))
                except OSError:
                    return False
        return True

    def mouse(cb, x, y, up=False):
        os.write(fd, f"\x1b[<{cb};{x + 1};{y + 1}{'m' if up else 'M'}".encode())

    pump(2.0)
    for step in spec["steps"]:
        kind = step[0]
        if kind == "shot":
            print(f"----- {step[1]}")
            print("\n".join(show(screen)))
            continue
        if kind == "key":
            os.write(fd, step[1].encode())
        elif kind == "mouse":
            mouse(step[1], step[2], step[3], step[4])
        elif kind == "drag":
            _, x0, y0, x1, y1 = step
            mouse(0, x0, y0)
            pump(0.1)
            n = max(abs(x1 - x0), abs(y1 - y0), 1)
            for i in range(1, n + 1):
                mouse(32, x0 + (x1 - x0) * i // n, y0 + (y1 - y0) * i // n)
            pump(0.2)
            mouse(0, x1, y1, True)
        elif kind == "sleep":
            pump(step[1])
            continue
        elif kind == "resize":
            _, c, r = step
            fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", r, c, 0, 0))
            screen.resize(r, c)
            os.kill(pid, signal.SIGWINCH)
        elif kind == "wait_exit":
            end = time.time() + step[1]
            while time.time() < end:
                done, status = os.waitpid(pid, os.WNOHANG)
                if done:
                    print(f"----- exited status={status}")
                    return
                pump(0.1)
            print("----- still running")
            continue
        pump()


if __name__ == "__main__":
    main()
