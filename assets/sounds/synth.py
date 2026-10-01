"""Sintetiza los sonidos de Lulo. Todo es generado aquí (sin muestras ajenas),
así que no hay licencias que respetar."""
import os, wave
import numpy as np

SR = 44100
OUT = os.path.join(os.path.dirname(__file__), "wav")
os.makedirs(OUT, exist_ok=True)


def t(d):
    return np.arange(int(SR * d)) / SR


def note(name):
    names = {"C": -9, "D": -7, "E": -5, "F": -4, "G": -2, "A": 0, "B": 2}
    n = names[name[0]]
    i = 1
    if name[1] in "#b":
        n += 1 if name[1] == "#" else -1
        i = 2
    octave = int(name[i:])
    return 440.0 * 2 ** ((n + 12 * (octave - 4)) / 12)


def env(x, attack=0.003):
    a = int(SR * attack)
    e = np.ones_like(x)
    e[:a] = np.linspace(0, 1, a)
    return e


def marimba(f, d=0.6, amp=1.0):
    x = t(d)
    s = (np.sin(2 * np.pi * f * x) * np.exp(-x * 7)
         + 0.35 * np.sin(2 * np.pi * f * 3.93 * x) * np.exp(-x * 30)
         + 0.12 * np.sin(2 * np.pi * f * 9.2 * x) * np.exp(-x * 60))
    return amp * s * env(x, 0.002)


def bell(f, d=1.2, amp=1.0, decay=3.5):
    x = t(d)
    parts = [(1, 1.0, 1), (2.0, 0.45, 1.6), (2.76, 0.35, 2.2), (5.4, 0.18, 4), (8.93, 0.08, 6)]
    s = sum(a * np.sin(2 * np.pi * f * r * x) * np.exp(-x * decay * k) for r, a, k in parts)
    return amp * s * env(x, 0.004)


def bubble(f0, f1, d=0.16, amp=1.0):
    """Burbuja: un seno cuyo tono sube (o baja) rápido con caída suave."""
    x = t(d)
    f = f0 * (f1 / f0) ** (x / d)
    ph = 2 * np.pi * np.cumsum(f) / SR
    e = np.sin(np.pi * np.clip(x / d, 0, 1)) ** 1.5
    return amp * np.sin(ph) * e


def thump(f=110, d=0.35, amp=1.0):
    x = t(d)
    f_sweep = f * (1 + 1.5 * np.exp(-x * 40))
    ph = 2 * np.pi * np.cumsum(f_sweep) / SR
    rng = np.random.default_rng(1)
    noise = rng.normal(0, 1, len(x)) * np.exp(-x * 90) * 0.25
    return amp * (np.sin(ph) * np.exp(-x * 12) + noise) * env(x, 0.001)


def mix(length, *events):
    out = np.zeros(int(SR * length))
    for start, sig in events:
        i = int(SR * start)
        j = min(len(out), i + len(sig))
        out[i:j] += sig[: j - i]
    return out


def reverb(s, wet=0.18):
    out = s.copy()
    # Filtros peine vectorizados por bloques (rápido y suficiente para clips cortos).
    tail = int(SR * 0.6)
    padded = np.concatenate([s, np.zeros(tail)])
    acc = np.zeros_like(padded)
    for delay, g in [(0.0297, 0.62), (0.0371, 0.58), (0.0411, 0.55), (0.0437, 0.5)]:
        n = int(SR * delay)
        y = padded.copy()
        for start in range(n, len(y), n):
            end = min(len(y), start + n)
            y[start:end] += g * y[start - n : end - n]
        acc += y
    acc /= 4
    return np.concatenate([out, np.zeros(tail)]) * (1 - wet) + acc * wet


def save(name, s, peak_db=-3.0):
    s = reverb(s)
    # Recorta silencio final.
    thresh = np.max(np.abs(s)) * 0.002
    last = np.nonzero(np.abs(s) > thresh)[0][-1]
    s = s[: last + 1]
    fade = int(SR * 0.02)
    s[-fade:] *= np.linspace(1, 0, fade)
    s = s / np.max(np.abs(s)) * 10 ** (peak_db / 20)
    data = (s * 32767).astype(np.int16)
    with wave.open(os.path.join(OUT, name + ".wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(data.tobytes())
    print(name, f"{len(s)/SR:.2f}s")


N = note
# Esperando: tiene que llamar la atención sin asustar.
save("esperando-toc-toc", mix(0.8, (0, marimba(N("G5"), amp=0.8)), (0.15, marimba(N("G5")))))
save("esperando-burbujas", mix(0.6, (0, bubble(500, 1300)), (0.13, bubble(700, 1800))), -6)
save("esperando-pregunta", mix(1.3, (0, bell(N("E6"), 1.0, 0.8)), (0.18, bell(N("A6"), 1.1))), -4)
# Terminó: alegre y breve.
save("termino-arpegio", mix(1.0, *[(i * 0.075, marimba(N(n), 0.7, 0.8 + 0.07 * i))
                                    for i, n in enumerate(["C5", "E5", "G5", "C6"])]))
save("termino-plop", mix(1.2, (0, bubble(350, 1100, 0.12)), (0.09, bell(N("C7"), 1.0, 0.35, 4.5))))
save("termino-campana", bell(N("A5"), 1.4, 1.0, 2.5), -5)
# Error: grave y descendente, sin sonar a alarma.
save("error-dos-notas", mix(0.9, (0, marimba(N("E4"), 0.6)), (0.17, marimba(N("C4"), 0.7))))
save("error-glub", mix(0.6, (0, bubble(700, 220, 0.22)), (0.2, bubble(420, 150, 0.25, 0.8))), -6)
save("error-golpe", mix(0.6, (0, thump(98)), (0.14, thump(82, amp=0.7))))
# Nueva sesión: casi un susurro (apagado por defecto).
save("nueva-burbuja", bubble(600, 1400, 0.11), -8)
save("nueva-gota", mix(0.6, (0, marimba(N("C6"), 0.5, 0.6))), -8)
