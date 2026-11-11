# The buildings of the Bourg Bouillotte (issue #245), assembled from the kit's pieces. Their
# front (+Z) is the side the game turns toward the track: the door, the shop windows.


def hip_roof(cells_x, cells_z, y, front_window=False, high=False):
    """A hipped roof over a box of cells: a corner piece at each corner, a slope along each edge
    between them (ridge along the longer side). `front_window`: a dormer in the middle of the
    front slope."""
    corner = 'roof-high-corner' if high else 'roof-corner'
    slope = 'roof-high' if high else 'roof'
    x0 = -(cells_x - 1) / 2
    z0 = -(cells_z - 1) / 2
    x1 = x0 + cells_x - 1
    z1 = z0 + cells_z - 1
    out = [(corner, x0, y, z1, 0), (corner, x1, y, z1, 90), (corner, x1, y, z0, 180),
           (corner, x0, y, z0, 270)]
    for i in range(1, cells_x - 1):
        x = x0 + i
        front = 'roof-window' if front_window and i == cells_x // 2 and not high else slope
        out.append((front, x, y, z1, 90))
        out.append((slope, x, y, z0, 270))
    for j in range(1, cells_z - 1):
        z = z0 + j
        out.append((slope, x0, y, z, 0))
        out.append((slope, x1, y, z, 180))
    return out


def floor_walls(cells_x, cells_z, y, front, sides, back):
    """One floor: `front`, `sides` and `back` each a function of the cell's index (or a name)."""
    pick = lambda f, i: f(i) if callable(f) else f
    def wall_for(edge, i, _floor):
        if edge == '+z':
            return pick(front, i)
        if edge == '-z':
            return pick(back, i)
        return pick(sides, i)
    return box(cells_x, cells_z, 0, wall_for, y)


# A half-timbered house, 2 x 2, two floors: a stone ground floor (the door, a window), a
# timbered floor above (two shuttered windows), a hipped roof and a chimney. The details on
# its front only (the side the game turns toward the track): plain walls elsewhere, fewer
# triangles.
SPECS['house-timber'] = (
    floor_walls(2, 2, 0, lambda i: 'wall-doorway-square' if i == 0 else 'wall-window-small',
                'wall', 'wall')
    + floor_walls(2, 2, 1, 'wall-wood-window-shutters', 'wall-wood', 'wall-wood')
    + hip_roof(2, 2, 2)
    + [('chimney-top', 0.5, 2.25, -0.5, 0)]
)

# A long low house, 3 x 2, one stone floor (a round doorway in the middle, two windows), a
# hipped roof with a dormer.
SPECS['house-long'] = (
    floor_walls(3, 2, 0, lambda i: 'wall-doorway-round' if i == 1 else 'wall-window-small',
                'wall', 'wall')
    + hip_roof(3, 2, 1, front_window=True)
)

# A tall house, 2 x 2, three floors: stone, timber, timber, a steep red roof, a chimney.
SPECS['house-tall'] = (
    floor_walls(2, 2, 0, lambda i: 'wall-doorway-square' if i == 1 else 'wall-window-small',
                'wall', 'wall')
    + floor_walls(2, 2, 1, 'wall-wood-window-glass', 'wall-wood-detail-diagonal', 'wall-wood')
    + floor_walls(2, 2, 2, 'wall-wood-window-shutters', 'wall-wood', 'wall-wood')
    + hip_roof(2, 2, 3, high=True)
    + [('chimney-top', -0.5, 3.3, 0.5, 180)]
)

# The water mill by the washhouse: a timbered house, 2 x 2, its wheel on its right side (+X).
SPECS['watermill-house'] = (
    floor_walls(2, 2, 0, lambda i: 'wall-wood-doorway-square' if i == 0 else 'wall-wood-window-shutters',
                'wall-wood', 'wall-wood')
    + hip_roof(2, 2, 1)
    + [('watermill', 1.45, 0.9, 0.0, 0)]
)

# The belfry: a slim stone tower, 1 x 1, six floors, its bell window and its banner near the
# top, a steep roof.
SPECS['belfry'] = (
    [w for y in range(4) for w in floor_walls(1, 1, y, 'wall-window-small' if y % 2 else 'wall',
                                               'wall', 'wall')]
    + floor_walls(1, 1, 4, 'wall-window-stone', 'wall-window-stone', 'wall-window-stone')
    + floor_walls(1, 1, 5, 'wall-doorway-round', 'wall-doorway-round', 'wall-doorway-round')
    + [('banner-red', 0, 3.0, 0.1, 270), ('roof-high-point', 0, 6, 0, 0)]
)

# The windmill on the hills: a stone tower, 1 x 1, three floors, a steep roof, its sails on
# its front.
SPECS['windmill-tower'] = (
    floor_walls(1, 1, 0, 'wall-doorway-square', 'wall', 'wall')
    + floor_walls(1, 1, 1, 'wall-window-small', 'wall', 'wall')
    + floor_walls(1, 1, 2, 'wall-wood-window-glass', 'wall-wood', 'wall-wood')
    + [('roof-high-point', 0, 3, 0, 0), ('windmill', 0, 2.5, 0.75, 90)]
)

# A little garden: a square of hedges round a tree, its gate on the front.
SPECS['garden'] = (
    [('hedge-large', 0, 0, 0, 0), ('hedge-large', 0, 0, 0, 90), ('hedge-large', 0, 0, 0, 180)]
    + [('hedge-large-gate', 0, 0, 0, 270), ('tree-high-round', 0, 0, 0, 0)]
)
