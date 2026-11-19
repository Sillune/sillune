function readMesh(source) {
  const data = Uint8Array.from(atob(source.slice(source.indexOf(",") + 1)), c => c.charCodeAt(0));
  if (data.length < 28) throw new Error("GLB incomplet");
  const header = new DataView(data.buffer);
  const u32 = at => header.getUint32(at, true);
  const jsonEnd = 20 + u32(12);
  if (u32(0) !== 0x46546c67 || u32(4) !== 2 || u32(8) !== data.length ||
      u32(16) !== 0x4e4f534a || jsonEnd + 8 > data.length || u32(jsonEnd + 4) !== 0x004e4942) {
    throw new Error("GLB invalide");
  }
  const binary = jsonEnd + 8;
  const doc = JSON.parse(new TextDecoder().decode(data.subarray(20, jsonEnd)));
  const primitive = doc.meshes[0].primitives[0];
  function accessor(index, components, type, ArrayType) {
    const acc = doc.accessors[index];
    const view = doc.bufferViews[acc.bufferView];
    const start = (view.byteOffset ?? 0) + (acc.byteOffset ?? 0);
    const size = acc.count * components * ArrayType.BYTES_PER_ELEMENT;
    if (view.buffer !== 0 || view.byteStride || acc.sparse || acc.componentType !== type ||
        !Number.isSafeInteger(start) || start < 0 || !Number.isSafeInteger(size) || size <= 0 ||
        start + size > data.length - binary) throw new Error("Attribut de maillage invalide");
    return new ArrayType(data.buffer, binary + start, acc.count * components);
  }
  const position = accessor(primitive.attributes.POSITION, 3, 5126, Float32Array);
  const indices = accessor(primitive.indices, 1, 5125, Uint32Array);
  const color = primitive.attributes.COLOR_0 === undefined ? null :
    accessor(primitive.attributes.COLOR_0, 3, 5126, Float32Array);
  if (color && color.length !== position.length) throw new Error("Couleurs invalides");
  const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < position.length; i++) {
    const axis = i % 3;
    min[axis] = Math.min(min[axis], position[i]);
    max[axis] = Math.max(max[axis], position[i]);
  }
  const extent = Math.max(...max.map((v, i) => v - min[i]));
  if (!Number.isFinite(extent) || extent <= 0) throw new Error("Dimensions invalides");
  const center = max.map((v, i) => (v + min[i]) / 2);
  for (let i = 0; i < position.length; i++) position[i] = (position[i] - center[i % 3]) / extent;
  return {position, indices, color};
}

export function showMesh(canvas, source) {
  const {position, indices, color} = readMesh(source);
  const gl = canvas.getContext("webgl2", {antialias: true});
  if (!gl) throw new Error("WebGL 2 indisponible");
  const program = gl.createProgram();
  function shader(type, source) {
    const code = gl.createShader(type);
    gl.shaderSource(code, source);
    gl.compileShader(code);
    if (!gl.getShaderParameter(code, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(code));
    gl.attachShader(program, code);
    gl.deleteShader(code);
  }
  shader(gl.VERTEX_SHADER, `#version 300 es
    layout(location=0) in vec3 position;
    layout(location=1) in vec3 tint;
    uniform vec4 camera;
    out vec3 surface;
    out vec3 rgba;
    void main() {
      float a=cos(camera.x), b=sin(camera.x), c=cos(camera.y), d=sin(camera.y);
      vec3 p=vec3(a*position.x+b*position.z,position.y,-b*position.x+a*position.z);
      surface=vec3(p.x,c*p.y-d*p.z,d*p.y+c*p.z);
      float w=1.9-surface.z;
      gl_Position=vec4(surface.x*camera.z/camera.w,surface.y*camera.z,-surface.z,w);
      rgba=tint;
    }`);
  shader(gl.FRAGMENT_SHADER, `#version 300 es
    precision highp float;
    in vec3 surface;
    in vec3 rgba;
    out vec4 pixel;
    void main() {
      vec3 normal=normalize(cross(dFdx(surface),dFdy(surface)));
      float light=.58+.42*abs(dot(normal,normalize(vec3(-.35,.7,1.))));
      pixel=vec4(pow(max(rgba*light,vec3(0.)),vec3(.8)),1.);
    }`);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
  gl.useProgram(program);
  function bind(target, values) {
    gl.bindBuffer(target, gl.createBuffer());
    gl.bufferData(target, values, gl.STATIC_DRAW);
  }
  function attribute(slot, data) {
    bind(gl.ARRAY_BUFFER, data);
    gl.enableVertexAttribArray(slot);
    gl.vertexAttribPointer(slot, 3, gl.FLOAT, false, 0, 0);
  }
  attribute(0, position);
  if (color) attribute(1, color); else gl.vertexAttrib3f(1, .60, .46, .90);
  bind(gl.ELEMENT_ARRAY_BUFFER, indices);
  const camera = gl.getUniformLocation(program, "camera");
  let yaw = -.45, pitch = -.35, zoom = 2.5;
  function draw() {
    const scale = Math.min(devicePixelRatio, 2);
    const width = Math.max(1, Math.round(canvas.clientWidth * scale));
    const height = Math.max(1, Math.round(canvas.clientHeight * scale));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    gl.viewport(0, 0, width, height);
    gl.clearColor(.075, .062, .104, 1);
    gl.clearDepth(1);
    gl.enable(gl.DEPTH_TEST);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.uniform4f(camera, yaw, pitch, zoom, width / height);
    gl.drawElements(gl.TRIANGLES, indices.length, gl.UNSIGNED_INT, 0);
  }
  const changeZoom = ratio => { zoom = Math.min(12, Math.max(1, zoom * ratio)); };
  canvas.onpointerdown = event => canvas.setPointerCapture(event.pointerId);
  canvas.onpointermove = event => {
    if (!canvas.hasPointerCapture(event.pointerId)) return;
    yaw += event.movementX * .012;
    pitch += event.movementY * .012;
    draw();
  };
  canvas.addEventListener("wheel", event => {
    event.preventDefault();
    changeZoom(Math.exp(-event.deltaY * .001));
    draw();
  }, {passive: false});
  canvas.onkeydown = event => {
    switch (event.key) {
      case "ArrowLeft": yaw -= .2; break;
      case "ArrowRight": yaw += .2; break;
      case "ArrowUp": pitch -= .2; break;
      case "ArrowDown": pitch += .2; break;
      case "+": changeZoom(1.15); break;
      case "-": changeZoom(1 / 1.15); break;
      default: return;
    }
    event.preventDefault();
    draw();
  };
  draw();
}
