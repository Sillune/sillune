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
  function accessor(index, components, types) {
    const acc = doc.accessors[index];
    const view = doc.bufferViews[acc.bufferView];
    const arrays = {5121: Uint8Array, 5123: Uint16Array, 5125: Uint32Array, 5126: Float32Array};
    const ArrayType = arrays[acc.componentType];
    if (!types.includes(acc.componentType)) throw new Error("Type d’attribut invalide");
    const start = (view.byteOffset ?? 0) + (acc.byteOffset ?? 0);
    const size = acc.count * components * ArrayType.BYTES_PER_ELEMENT;
    if (view.buffer !== 0 || view.byteStride || acc.sparse ||
        !Number.isSafeInteger(start) || start < 0 || !Number.isSafeInteger(size) || size <= 0 ||
        start + size > data.length - binary ||
        (acc.byteOffset ?? 0) + size > view.byteLength) throw new Error("Attribut de maillage invalide");
    return new ArrayType(data.buffer, binary + start, acc.count * components);
  }
  const attributes = primitive.attributes;
  const position = accessor(attributes.POSITION, 3, [5126]);
  const indices = accessor(primitive.indices, 1, [5121, 5123, 5125]);
  const vertexCount = position.length / 3;
  const optional = (name, components, types) => {
    if (attributes[name] === undefined) return null;
    const values = accessor(attributes[name], components, types);
    if (values.length !== vertexCount * components) throw new Error("Attribut de taille incorrecte");
    return values;
  };
  const color = optional("COLOR_0", 3, [5126]);
  const normal = optional("NORMAL", 3, [5126]);
  const uv = optional("TEXCOORD_0", 2, [5126]);
  const material = doc.materials?.[primitive.material]?.pbrMetallicRoughness;
  const texture = doc.textures?.[material?.baseColorTexture?.index];
  const image = doc.images?.[texture?.extensions?.EXT_texture_webp?.source ?? texture?.source];
  let bitmap = null;
  if (image && uv) {
    const view = doc.bufferViews[image.bufferView];
    const start = (view.byteOffset ?? 0) + binary;
    const end = start + view.byteLength;
    if (view.buffer !== 0 || !["image/png", "image/jpeg", "image/webp"].includes(image.mimeType) ||
        start < binary || end > data.length || end <= start) throw new Error("Texture GLB invalide");
    bitmap = new Blob([data.subarray(start, end)], {type: image.mimeType});
  }
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
  return {position, indices, color, normal, uv, bitmap, factor: material?.baseColorFactor};
}

export async function showMesh(canvas, source) {
  const {position, indices, color, normal, uv, bitmap, factor} = readMesh(source);
  const image = bitmap && await createImageBitmap(bitmap, {colorSpaceConversion: "none"});
  if (!canvas.isConnected) { image?.close(); return; }
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
    layout(location=2) in vec3 vertexNormal;
    layout(location=3) in vec2 uv;
    uniform vec4 camera;
    out vec3 surface;
    out vec3 rgba;
    out vec3 smoothNormal;
    out vec2 texcoord;
    vec3 rotate(vec3 v, float a, float b, float c, float d) {
      vec3 p=vec3(a*v.x+b*v.z,v.y,-b*v.x+a*v.z);
      return vec3(p.x,c*p.y-d*p.z,d*p.y+c*p.z);
    }
    void main() {
      float a=cos(camera.x), b=sin(camera.x), c=cos(camera.y), d=sin(camera.y);
      surface=rotate(position,a,b,c,d);
      smoothNormal=rotate(vertexNormal,a,b,c,d);
      float w=1.9-surface.z;
      gl_Position=vec4(surface.x*camera.z/camera.w,surface.y*camera.z,-surface.z,w);
      rgba=tint;
      texcoord=uv;
    }`);
  shader(gl.FRAGMENT_SHADER, `#version 300 es
    precision highp float;
    in vec3 surface;
    in vec3 rgba;
    in vec3 smoothNormal;
    in vec2 texcoord;
    uniform sampler2D albedo;
    uniform bool textured;
    out vec4 pixel;
    void main() {
      vec3 n=length(smoothNormal)>.01 ? smoothNormal : cross(dFdx(surface),dFdy(surface));
      float light=.61+.39*abs(dot(normalize(n),normalize(vec3(-.35,.7,1.))));
      vec3 base=textured ? texture(albedo,texcoord).rgb*rgba : rgba;
      pixel=vec4(pow(max(base*light,vec3(0.)),vec3(.4545)),1.);
    }`);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
  gl.useProgram(program);
  function bind(target, values) {
    gl.bindBuffer(target, gl.createBuffer());
    gl.bufferData(target, values, gl.STATIC_DRAW);
  }
  function attribute(slot, values, components) {
    bind(gl.ARRAY_BUFFER, values);
    gl.enableVertexAttribArray(slot);
    gl.vertexAttribPointer(slot, components, gl.FLOAT, false, 0, 0);
  }
  attribute(0, position, 3);
  if (color) attribute(1, color, 3);
  else gl.vertexAttrib3f(1, ...(factor?.slice(0, 3) ?? (image ? [1, 1, 1] : [.4, .28, .75])));
  if (normal) attribute(2, normal, 3);
  if (uv) attribute(3, uv, 2);
  bind(gl.ELEMENT_ARRAY_BUFFER, indices);
  const indexType = {1: gl.UNSIGNED_BYTE, 2: gl.UNSIGNED_SHORT, 4: gl.UNSIGNED_INT}[indices.BYTES_PER_ELEMENT];
  const surfaceTexture = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, surfaceTexture);
  if (image) {
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.SRGB8_ALPHA8, gl.RGBA, gl.UNSIGNED_BYTE, image);
    image.close();
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.uniform1i(gl.getUniformLocation(program, "textured"), 1);
  } else {
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([255, 255, 255, 255]));
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  }
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
    gl.drawElements(gl.TRIANGLES, indices.length, indexType, 0);
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
