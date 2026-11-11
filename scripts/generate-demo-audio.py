"""Generate the bundled, original ambient loops; no third-party recordings."""
from array import array
from math import cos, exp, log2, pi, sin, sqrt, tanh
from pathlib import Path
import random
import sys
import wave

out = Path(__file__).resolve().parents[1] / "wwwroot" / "audio"
out.mkdir(exist_ok=True)


def visualizer_spectrum():
    window_size = 2048
    window = [(0.5 - 0.5 * cos(2 * pi * index / (window_size - 1))) / 32768
              for index in range(window_size)]
    bits = window_size.bit_length() - 1
    order = [int(f"{index:0{bits}b}"[::-1], 2) for index in range(window_size)]
    roots = [complex(cos(-2 * pi * index / window_size), sin(-2 * pi * index / window_size))
             for index in range(window_size // 2)]
    spectrum = bytearray()
    for index in range(1, 7):
        with wave.open(str(out / f"sample-{index}.wav"), "rb") as source:
            rate = source.getframerate()
            samples = array("h", [0] * (window_size // 2))
            samples.frombytes(source.readframes(source.getnframes()))
            if sys.byteorder != "little":
                samples.byteswap()
        samples.extend([0] * (540 * (rate // 20) + window_size - len(samples)))
        # Geometric spacing keeps bass and treble legible in the same strip.
        first_bin = max(1, round(31 * window_size / rate))
        last_bin = min(window_size // 2, 7600 * window_size // rate)
        edges = [first_bin]
        for band in range(1, 65):
            edge = round(first_bin * (last_bin / first_bin) ** (band / 64))
            edges.append(max(edges[-1] + 1, edge))
        bands = [(start, end, 12 + 6 * log2(start / first_bin))
                 for start, end in zip(edges, edges[1:])]
        for frame in range(540):
            start = frame * (rate // 20)
            bins = [samples[start + offset] * window[offset] for offset in order]
            step = 2
            while step <= window_size:
                half = step // 2
                for base in range(0, window_size, step):
                    for offset in range(half):
                        a = base + offset
                        b = a + half
                        rotated = bins[b] * roots[offset * (window_size // step)]
                        bins[a], bins[b] = bins[a] + rotated, bins[a] - rotated
                step *= 2
            for start, end, gain in bands:
                power = sum(abs(value) ** 2 for value in bins[start:end])
                magnitude = sqrt(power) * 4 / window_size
                spectrum.append(round((1 - exp(-magnitude * gain)) * 255))
    output = out.parents[1] / "src" / "shaders" / "demo-spectrum.bin"
    output.write_bytes(spectrum)


if "--spectrum-only" in sys.argv:
    visualizer_spectrum()
    sys.exit()

rate = 16000
for index, (root, beat, seconds) in enumerate([(48, .62, 24), (45, .71, 26), (52, .49, 22),
                                               (43, .81, 27), (50, .57, 25), (46, .66, 24)]):
    rng = random.Random(20 + index)
    audio = array("h")
    chords = [[0, 3, 7, 14], [-4, 0, 3, 10], [-2, 2, 5, 12], [-7, 0, 2, 8]]
    freqs = [[440 * 2 ** ((root + d - 69) / 12) for d in chord] for chord in chords]
    for i in range(rate * seconds):
        t = i / rate
        b = t / beat
        phase = t % beat
        chord = freqs[int(b / 4) % 4]
        prev = freqs[(int(b / 4) - 1) % 4]
        mix = min(1, (b % 4) * 3)
        pad = sum((mix * sin(2*pi*f*t + .25*sin(t*(j+1)*.8))
                   + (1-mix) * sin(2*pi*p*t + .25*sin(t*(j+1)*.8))) / (j+2)
                  for j, (f,p) in enumerate(zip(chord, prev)))
        arp_f = chord[int(b * 2) % 4] * 2
        tap = phase % (beat / 2)
        arp = (sin(2*pi*arp_f*t) + .16*sin(4*pi*arp_f*t)) * exp(-tap*16) * min(1,tap*130)
        kick = sin(2*pi*(43*phase + 1.9*(1-exp(-27*phase)))) * exp(-phase*17)
        hat = rng.uniform(-1, 1) * exp(-tap*95)
        envelope = min(1, t*1.5, (seconds-t)*1.2)
        value = tanh(.31*pad + .13*arp + .17*kick + .075*hat) * envelope
        audio.append(round(value * 27000))
    if sys.byteorder != "little":
        audio.byteswap()
    with wave.open(str(out / f"sample-{index+1}.wav"), "wb") as output:
        output.setparams((1, 2, rate, 0, "NONE", "not compressed"))
        output.writeframes(audio.tobytes())

visualizer_spectrum()
