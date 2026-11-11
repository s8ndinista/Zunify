"""Generate the bundled, original ambient loops; no third-party recordings."""
from array import array
from math import exp, pi, sin, tanh
from pathlib import Path
import random
import sys
import wave

out = Path(__file__).resolve().parents[1] / "wwwroot" / "audio"
out.mkdir(exist_ok=True)
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
