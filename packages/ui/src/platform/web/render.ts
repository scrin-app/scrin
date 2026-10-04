/**
 * Draws decoded `VideoFrame`s onto a canvas: WebGL2 (`texImage2D` straight
 * from the frame, no CPU copy) with a 2D `drawImage` fallback. Latest frame
 * wins; every frame is closed as soon as it is drawn or superseded.
 */

export interface FrameRenderer {
  readonly kind: 'webgl2' | '2d';
  submit(frame: VideoFrame): void;
  dispose(): void;
}

// One oversized triangle covering the viewport; uv (0,0) = top-left of the frame.
const VS = `#version 300 es
out vec2 uv;
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  uv = vec2(p.x, 1.0 - p.y);
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

const FS = `#version 300 es
precision mediump float;
in vec2 uv;
uniform sampler2D tex;
out vec4 color;
void main() { color = texture(tex, uv); }`;

function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader | null {
  const s = gl.createShader(type);
  if (!s) return null;
  gl.shaderSource(s, src);
  gl.compileShader(s);
  return gl.getShaderParameter(s, gl.COMPILE_STATUS) ? s : null;
}

function webgl2(canvas: HTMLCanvasElement): ((f: VideoFrame) => void) | null {
  const gl = canvas.getContext('webgl2', {
    alpha: false,
    antialias: false,
    desynchronized: true,
    powerPreference: 'high-performance',
  });
  if (!gl) return null;
  const vs = compile(gl, gl.VERTEX_SHADER, VS);
  const fs = compile(gl, gl.FRAGMENT_SHADER, FS);
  const prog = gl.createProgram();
  if (!vs || !fs) return null;
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return null;
  const tex = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, tex);
  for (const p of [gl.TEXTURE_WRAP_S, gl.TEXTURE_WRAP_T])
    gl.texParameteri(gl.TEXTURE_2D, p, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.useProgram(prog);
  return (f) => {
    if (gl.isContextLost()) return;
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, f);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };
}

function canvas2d(canvas: HTMLCanvasElement): ((f: VideoFrame) => void) | null {
  const ctx = canvas.getContext('2d', { alpha: false, desynchronized: true });
  if (!ctx) return null;
  return (f) => {
    ctx.drawImage(f, 0, 0, canvas.width, canvas.height);
  };
}

export function createRenderer(canvas: HTMLCanvasElement): FrameRenderer | null {
  const gl = webgl2(canvas);
  const draw = gl ?? canvas2d(canvas);
  if (!draw) return null;
  let pending: VideoFrame | null = null;
  let raf = 0;
  let disposed = false;
  const paint = () => {
    raf = 0;
    const f = pending;
    pending = null;
    if (!f) return;
    try {
      if (canvas.width !== f.displayWidth || canvas.height !== f.displayHeight) {
        canvas.width = f.displayWidth;
        canvas.height = f.displayHeight;
      }
      draw(f);
    } finally {
      f.close();
    }
  };
  return {
    kind: gl ? 'webgl2' : '2d',
    submit(f) {
      if (disposed) {
        f.close();
        return;
      }
      pending?.close();
      pending = f;
      raf ||= requestAnimationFrame(paint);
    },
    dispose() {
      disposed = true;
      if (raf) cancelAnimationFrame(raf);
      pending?.close();
      pending = null;
    },
  };
}
