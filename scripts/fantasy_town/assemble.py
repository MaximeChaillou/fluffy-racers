"""Assembles models of Kenney's Fantasy Town Kit (CC0) into whole buildings (issue #245).

Each building is a list of the kit's pieces, each moved (x, y, z, in the kit's units: a cell is
1 x 1 x 1) and turned around the vertical (degrees). The pieces are copied as they are (their
meshes, their accessors, their one shared material and its colormap), each under a node of its
own, under one root: one .glb per building, that points at `Textures/colormap.png` like the
kit's own files.

Usage: python3 -I scripts/fantasy_town/assemble.py "<kit>/Models/GLB format" assets/models/fantasy-town-kit
(the buildings are in `buildings.py`, next to this file; the kit's own pieces are copied by hand).
"""

import json
import math
import os
import struct
import sys

KIT = sys.argv[1]
OUT = sys.argv[2]


def load(name):
    b = open(os.path.join(KIT, name + '.glb'), 'rb').read()
    _, _, length = struct.unpack('<III', b[:12])
    off = 12
    js = None
    binc = b''
    while off < length:
        clen, ctype = struct.unpack('<II', b[off:off + 8])
        data = b[off + 8:off + 8 + clen]
        if ctype == 0x4E4F534A:
            js = json.loads(data)
        else:
            binc = data
        off += 8 + clen
    return js, binc


def quat_y(deg):
    a = math.radians(deg) / 2
    return [0.0, math.sin(a), 0.0, math.cos(a)]


def build(pieces):
    out = {
        'asset': {'version': '2.0', 'generator': 'fluffy-racers assemble.py (Fantasy Town Kit)'},
        'scene': 0,
        'scenes': [{'nodes': [0]}],
        'nodes': [{'name': 'root', 'children': []}],
        'meshes': [],
        'accessors': [],
        'bufferViews': [],
        'buffers': [{'byteLength': 0}],
        'materials': [],
        'textures': [],
        'images': [],
        'samplers': [],
    }
    blob = bytearray()
    cache = {}
    mat_index = {}
    for name, x, y, z, yaw in pieces:
        if name not in cache:
            js, binc = load(name)
            # Buffer views.
            base_view = len(out['bufferViews'])
            while len(blob) % 4:
                blob.append(0)
            base_off = len(blob)
            blob.extend(binc)
            for view in js['bufferViews']:
                v = dict(view)
                v['buffer'] = 0
                v['byteOffset'] = v.get('byteOffset', 0) + base_off
                out['bufferViews'].append(v)
            base_acc = len(out['accessors'])
            for acc in js['accessors']:
                a = dict(acc)
                a['bufferView'] = a['bufferView'] + base_view
                out['accessors'].append(a)
            # Materials: one per name (the kit's "colormap", the fountains' "Water").
            mats = []
            for m in js.get('materials', []):
                key = m.get('name')
                if key not in mat_index:
                    m2 = json.loads(json.dumps(m))
                    pbr = m2.get('pbrMetallicRoughness', {})
                    if 'baseColorTexture' in pbr:
                        if not out['images']:
                            out['images'].append({'uri': 'Textures/colormap.png'})
                            out['samplers'].append(js['samplers'][0] if js.get('samplers') else {})
                            out['textures'].append({'source': 0, 'sampler': 0})
                        pbr['baseColorTexture'] = {'index': 0}
                    mat_index[key] = len(out['materials'])
                    out['materials'].append(m2)
                mats.append(mat_index[key])
            meshes = {}
            for mi, mesh in enumerate(js['meshes']):
                m2 = {'name': mesh.get('name', name), 'primitives': []}
                for prim in mesh['primitives']:
                    p = {'attributes': {k: v + base_acc for k, v in prim['attributes'].items()}}
                    if 'indices' in prim:
                        p['indices'] = prim['indices'] + base_acc
                    if 'material' in prim:
                        p['material'] = mats[prim['material']]
                    if 'mode' in prim:
                        p['mode'] = prim['mode']
                    m2['primitives'].append(p)
                meshes[mi] = len(out['meshes'])
                out['meshes'].append(m2)
            cache[name] = (js, meshes)
        js, meshes = cache[name]
        # The piece's node tree, under a node of its own.
        holder = len(out['nodes'])
        out['nodes'].append({'name': name, 'translation': [x, y, z], 'rotation': quat_y(yaw),
                             'children': []})
        out['nodes'][0]['children'].append(holder)

        def copy(n, parent):
            node = js['nodes'][n]
            n2 = {k: v for k, v in node.items() if k in ('name', 'translation', 'rotation', 'scale', 'matrix')}
            if 'mesh' in node:
                n2['mesh'] = meshes[node['mesh']]
            idx = len(out['nodes'])
            out['nodes'].append(n2)
            out['nodes'][parent].setdefault('children', []).append(idx)
            for c in node.get('children', []):
                copy(c, idx)

        for r in js['scenes'][js.get('scene', 0)]['nodes']:
            copy(r, holder)
    for n in out['nodes']:
        if 'children' in n and not n['children']:
            del n['children']
    for key in ('textures', 'images', 'samplers'):
        if not out[key]:
            del out[key]
    while len(blob) % 4:
        blob.append(0)
    out['buffers'][0]['byteLength'] = len(blob)
    js_bytes = json.dumps(out, separators=(',', ':')).encode()
    while len(js_bytes) % 4:
        js_bytes += b' '
    total = 12 + 8 + len(js_bytes) + 8 + len(blob)
    return (struct.pack('<III', 0x46546C67, 2, total)
            + struct.pack('<II', len(js_bytes), 0x4E4F534A) + js_bytes
            + struct.pack('<II', len(blob), 0x004E4942) + bytes(blob))


# ---- The buildings. A cell is 1 x 1 x 1; a wall stands on the +X edge of its cell (x 0.4 to
# 0.5), facing +X; turned by 90 degrees it stands on another edge. Fronts toward +Z (the game
# turns a model's +Z toward the track).
# Yaw (around +Y, counterclockwise seen from above): 0 -> +X edge, 90 -> -Z edge, 180 -> -X,
# 270 -> +Z edge.
EDGE = {'+x': 0, '-z': 90, '-x': 180, '+z': 270}


def box(cells_x, cells_z, floor, wall_for, y):
    """The walls of one floor of a box of cells (centred on the origin), each edge's piece given
    by `wall_for(edge, i)`."""
    out = []
    x0 = -(cells_x - 1) / 2
    z0 = -(cells_z - 1) / 2
    for i in range(cells_x):
        cx = x0 + i
        out.append((wall_for('+z', i, floor), cx, y, z0 + cells_z - 1, EDGE['+z']))
        out.append((wall_for('-z', i, floor), cx, y, z0, EDGE['-z']))
    for j in range(cells_z):
        cz = z0 + j
        out.append((wall_for('+x', j, floor), x0 + cells_x - 1, y, cz, EDGE['+x']))
        out.append((wall_for('-x', j, floor), x0, y, cz, EDGE['-x']))
    return out


SPECS = {}

os.makedirs(OUT, exist_ok=True)
exec(open(os.path.join(os.path.dirname(__file__), 'buildings.py')).read())
for name, pieces in SPECS.items():
    open(os.path.join(OUT, name + '.glb'), 'wb').write(build(pieces))
    print(name, len(pieces), 'pieces')
