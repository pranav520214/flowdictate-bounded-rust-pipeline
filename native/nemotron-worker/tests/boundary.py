"""Bounded, model-free tests of the production native adapter; no file writes."""
import struct
import subprocess
import sys


def u64(value):
    return struct.pack('<Q', value)


def u32(value):
    return struct.pack('<I', value)


def startup(name='expect0', version=1):
    value = name.encode('ascii')
    return b'FDNEMO01' + struct.pack('<H', version) + u32(len(value)) + value


def request(kind, identity):
    return bytes([kind]) + u64(identity)


def push(identity, value=0):
    return request(1, identity) + u32(1) + struct.pack('<f', value)


cases = 0


def check(payload, code, expected):
    global cases
    result = subprocess.run([sys.argv[1]], input=payload, capture_output=True,
                            timeout=5, creationflags=subprocess.CREATE_NO_WINDOW)
    # Never include captured output or payload in failure diagnostics.
    if result.returncode != code or result.stdout != expected or result.stderr:
        raise RuntimeError('native boundary result mismatch')
    cases += 1


ready = bytes([128])
none1, none2 = request(129, 1), request(129, 2)
error1 = request(131, 1)
check(startup(version=2), 1, bytes([132]))
check(bytes(14), 1, bytes([132]))
check(b'FDNEMO01' + struct.pack('<H', 1) + u32(32769), 1, bytes([132]))
check(startup() + request(99, 1), 1, ready)
check(startup() + request(1, 0), 1, ready)
check(startup() + request(1, 1) + u32(1), 1, ready)
for length in [0, 2561, 2**32 - 1]:
    check(startup() + request(1, 1) + u32(length), 1, ready + error1 + bytes([1]))
check(startup() + request(2, 1) + request(2, 2), 0, ready + none1 + none2)
check(startup() + request(2, 1) + request(2, 1), 1, ready + none1)
for value in [float('nan'), float('inf'), 2]:
    check(startup() + push(1, value), 1, ready + error1 + bytes([1]))
final2 = request(130, 2) + bytes([1]) + u32(0) + u32(0)
final4 = request(130, 4) + bytes([1]) + u32(0) + u32(0)
statistics = request(133, 5) + b''.join(u64(v) for v in [2, 2, 2, 0, 0, 1])
check(startup('expect2') + push(1) + request(2, 2) + push(3) + request(2, 4) + request(4, 5),
      0, ready + none1 + final2 + request(129, 3) + final4 + statistics)
check(startup('expect1') + push(1), 0, ready + none1)  # Parent pipes disappear.
check(startup('expect1') + push(1) + bytes([3]), 0, ready + none1)
check(startup('create-error') + push(1), 1, ready + error1 + bytes([3]))
check(startup('expect1') + push(1, -1), 1, ready + error1 + bytes([3]))
for value in [-0.5, 0.5, 0.25]:
    code = 4 if value == 0.25 else 3
    check(startup('expect1') + push(1, value) + request(2, 2),
          1, ready + none1 + request(131, 2) + bytes([code]))
print(f'boundary_cases={cases} failures=0')
