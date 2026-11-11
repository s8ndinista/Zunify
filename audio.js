// The page's audio path. Demo audio runs through a Web Audio graph instead of straight to the
// output device, so an analyser hears exactly what plays. Spotify's Web Playback SDK decrypts and
// plays its protected stream inside its own frame, out of reach of that graph; the only way a page
// can hear it is to capture the audio of its own tab, which the listener has to allow.
const size = 2048;

export class AudioPath {
  #element;
  #context;
  #gain;
  #local;
  #capture;
  #volume = 1;
  #buffer = new Float32Array(size);

  constructor(element) { this.#element = element; }

  get rate() { return this.#context?.sampleRate ?? 0; }
  get capturing() { return Boolean(this.#capture); }
  get canCapture() {
    return Boolean(navigator.mediaDevices?.getDisplayMedia && (globalThis.AudioContext || globalThis.webkitAudioContext));
  }

  #open() {
    if (this.#context !== undefined) return this.#context;
    this.#context = null;
    const Context = globalThis.AudioContext || globalThis.webkitAudioContext;
    if (!Context) return null;
    let context;
    try {
      context = new Context();
      const source = context.createMediaElementSource(this.#element);
      this.#local = this.#analyser(context);
      this.#gain = context.createGain();
      this.#gain.gain.value = this.#volume;
      // The analyser sits ahead of the volume control so the spectrum follows the music.
      source.connect(this.#local).connect(this.#gain).connect(context.destination);
      this.#element.volume = 1;
      this.#context = context;
    } catch {
      // Without Web Audio the element keeps playing directly; only the spectrum is lost.
      context?.close?.().catch(() => {});
    }
    return this.#context;
  }

  #analyser(context) {
    const analyser = context.createAnalyser();
    analyser.fftSize = size;
    return analyser;
  }

  // Browsers only start audio from a user gesture, so call this on the way to play().
  wake() {
    const context = this.#open();
    if (context && context.state !== "running") context.resume().catch(() => {});
  }

  set volume(value) {
    this.#volume = value;
    if (this.#gain) this.#gain.gain.value = value;
    else this.#element.volume = value;
  }

  // The latest window of samples from the local audio graph or, with `captured`, from the tab capture.
  // `gain` undoes a volume that was applied before the capture.
  samples(captured = false, gain = 1) {
    const analyser = captured ? this.#capture?.analyser : this.#local;
    if (!analyser) return null;
    analyser.getFloatTimeDomainData(this.#buffer);
    if (gain !== 1) for (let index = 0; index < size; index++) this.#buffer[index] *= gain;
    return this.#buffer;
  }

  // Asks the listener to share this tab's audio. Must run from a user gesture.
  async capture() {
    if (this.#capture) return;
    const context = this.canCapture && this.#open();
    if (!context) throw new Error("This browser cannot share tab audio with the app.");
    const stream = await navigator.mediaDevices.getDisplayMedia({
      video: { displaySurface: "browser" },
      audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false, suppressLocalAudioPlayback: false },
      preferCurrentTab: true, selfBrowserSurface: "include", systemAudio: "exclude",
      surfaceSwitching: "exclude", monitorTypeSurfaces: "exclude",
    });
    // Browsers insist on a video track. Only the audio is of use here.
    for (const track of stream.getVideoTracks()) track.stop();
    const [track] = stream.getAudioTracks();
    if (!track) throw new Error("The tab was shared without its audio. Try again and keep “Also share tab audio” on.");
    if (context.state !== "running") context.resume().catch(() => {});
    const source = context.createMediaStreamSource(new MediaStream([track]));
    const analyser = this.#analyser(context);
    // A muted route to the output keeps the analyser running without playing the tab twice.
    const mute = context.createGain();
    mute.gain.value = 0;
    source.connect(analyser).connect(mute).connect(context.destination);
    this.#capture = { track, source, analyser, mute };
    track.addEventListener("ended", () => this.release());
  }

  release() {
    const capture = this.#capture;
    if (!capture) return;
    this.#capture = undefined;
    capture.track.stop();
    capture.source.disconnect();
    capture.mute.disconnect();
    this.changed?.();
  }
}
