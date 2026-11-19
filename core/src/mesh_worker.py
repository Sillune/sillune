"""Convert a local Shap-E text prediction to an indexed, colored glTF mesh."""

import json
from pathlib import Path
import struct
import sys


def export_mesh(mesh, path):
    import numpy as np

    positions = mesh.verts.detach().cpu().float().numpy().astype("<f4")
    # Shap-E uses Z-up, glTF uses Y-up.
    positions = positions[:, [0, 2, 1]].copy()
    positions[:, 2] *= -1
    indices = mesh.faces.detach().cpu().numpy().astype("<u4").ravel()
    if not len(positions) or not len(indices) or indices.max() >= len(positions):
        raise ValueError("La génération a produit un maillage vide ou invalide.")
    if not np.isfinite(positions).all():
        raise ValueError("Le maillage contient des coordonnées invalides.")
    views, accessors = [], []
    data = bytearray()

    def add(array, shape, component, target):
        payload = array.tobytes()
        views.append({"buffer": 0, "byteOffset": len(data), "byteLength": len(payload), "target": target})
        data.extend(payload)
        accessors.append({"bufferView": len(views) - 1, "type": shape, "componentType": component,
                          "count": len(array)})
        return len(accessors) - 1

    attributes = {"POSITION": add(positions, "VEC3", 5126, 34962)}
    accessors[0].update(min=positions.min(axis=0).tolist(), max=positions.max(axis=0).tolist())
    index = add(indices, "SCALAR", 5125, 34963)
    channels = mesh.vertex_channels or {}
    if all(key in channels for key in ("R", "G", "B")):
        colors = np.stack([channels[key].detach().cpu().float().numpy() for key in ("R", "G", "B")], axis=-1)
        colors = np.nan_to_num(colors).clip(0, 1).astype("<f4")
        attributes["COLOR_0"] = add(colors.reshape(-1, 3), "VEC3", 5126, 34962)
    document = {"asset": {"version": "2.0", "generator": "Sillune + Shap-E"},
                "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
                "meshes": [{"primitives": [{"attributes": attributes, "indices": index}]}],
                "accessors": accessors, "bufferViews": views, "buffers": [{"byteLength": len(data)}]}
    gltf = json.dumps(document, separators=(",", ":")).encode()
    gltf += b" " * (-len(gltf) % 4)
    with Path(path).open("wb") as output:
        output.write(struct.pack("<4s4I", b"glTF", 2, 28 + len(gltf) + len(data),
                                 len(gltf), 0x4E4F534A))
        output.write(gltf)
        output.write(struct.pack("<2I", len(data), 0x004E4942))
        output.write(data)
    return len(positions), len(indices) // 3


def main():
    import torch
    from diffusers import ShapEPipeline
    from diffusers.pipelines.shap_e.renderer import MeshDecoderOutput, ShapERenderer
    from skimage.measure import marching_cubes

    class MarchingCubes(torch.nn.Module):
        # The FP16 safetensors omit the legacy marching cubes lookup tables.
        def forward(self, field, minimum, size):
            space = (size.detach().cpu() / (torch.tensor(field.shape) - 1)).tolist()
            verts, faces, _, _ = marching_cubes(
                field.detach().cpu().float().numpy(), level=0.0, spacing=space, allow_degenerate=False
            )
            verts = torch.from_numpy(verts.copy()).to(device=field.device, dtype=field.dtype)
            faces = torch.from_numpy(faces.copy()).to(device=field.device, dtype=torch.long)
            return MeshDecoderOutput(verts=verts + minimum.to(field.device), faces=faces, vertex_channels=None)

    request = json.load(sys.stdin)
    device = "xpu" if request["xpu"] and torch.xpu.is_available() else "cpu"
    print(f"SILLUNE_DEVICE={device}", file=sys.stderr, flush=True)
    dtype = torch.float16 if device == "xpu" else torch.float32
    if device == "cpu":
        import os

        torch.set_num_threads(min(8, os.cpu_count() or 1))
    options = {"variant": "fp16", "use_safetensors": True, "local_files_only": True, "torch_dtype": dtype}
    renderer = ShapERenderer.from_pretrained(Path(request["model"]) / "renderer", **options)
    renderer.mesh_decoder = MarchingCubes()
    pipeline = ShapEPipeline.from_pretrained(request["model"], shap_e_renderer=renderer, **options)
    pipeline.set_progress_bar_config(disable=True)
    pipeline.to(device)
    generator = torch.Generator(device="cpu").manual_seed(request["seed"])
    with torch.inference_mode():
        mesh = pipeline(request["prompt"], num_inference_steps=request["steps"], generator=generator,
                        output_type="mesh", guidance_scale=15.0).images[0]
    vertices, triangles = export_mesh(mesh, request["output"])
    print(json.dumps({"vertices": vertices, "triangles": triangles, "device": device}))


if __name__ == "__main__":
    main()
