"""Build public/fonts/tw-shapes.woff2: the geometric shapes the UI writes as
text (disclosure and menu carets, up/down markers). IBM Plex has none of
them, so without this every platform borrows a different system font and the
same caret changes size and shape between controls.

Plain polygons on IBM Plex's metrics (1000 upm, ascent 1025, descent 275),
600 wide like Plex Mono so tabular columns keep their alignment.

    python3 -I frontend/scripts/shapes-font.py   (needs fonttools and brotli)
"""
from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

ADVANCE = 600
MID_X, MID_Y = 300, 340


def area(points):
    return sum(x0 * y1 - x1 * y0 for (x0, y0), (x1, y1) in zip(points, points[1:] + points[:1]))


def contour(pen, points, hole=False):
    # TrueType fills clockwise outlines; holes run the other way.
    if (area(points) > 0) != hole:
        points = points[::-1]
    pen.moveTo(points[0])
    for point in points[1:]:
        pen.lineTo(point)
    pen.closePath()


def triangle(direction, width, height):
    w, h = width / 2, height / 2
    tip = {
        'up': [(MID_X - w, MID_Y - h), (MID_X, MID_Y + h), (MID_X + w, MID_Y - h)],
        'down': [(MID_X - w, MID_Y + h), (MID_X + w, MID_Y + h), (MID_X, MID_Y - h)],
        'right': [(MID_X - w, MID_Y - h), (MID_X - w, MID_Y + h), (MID_X + w, MID_Y)],
        'left': [(MID_X + w, MID_Y - h), (MID_X + w, MID_Y + h), (MID_X - w, MID_Y)],
    }[direction]
    return [(round(x), round(y)) for x, y in tip]


def diamond(half_w, half_h):
    return [(MID_X, MID_Y + half_h), (MID_X + half_w, MID_Y), (MID_X, MID_Y - half_h), (MID_X - half_w, MID_Y)]


def glyph(*contours):
    pen = TTGlyphPen(None)
    for points, hole in contours:
        contour(pen, points, hole)
    return pen.glyph()


LARGE = (520, 450)
SMALL_SIDE = (400, 460)
SMALL_DOWN = (460, 400)
STROKE = 70

shapes = {
    0x25B2: ('triUp', glyph((triangle('up', *LARGE), False))),
    0x25B3: ('triUpOutline', glyph((triangle('up', *LARGE), False),
                                   (triangle('up', LARGE[0] - 2.6 * STROKE, LARGE[1] - 2.2 * STROKE), True))),
    0x25B4: ('triUpSmall', glyph((triangle('up', *SMALL_DOWN), False))),
    0x25B6: ('triRight', glyph((triangle('right', LARGE[1], LARGE[0]), False))),
    0x25B8: ('triRightSmall', glyph((triangle('right', *SMALL_SIDE), False))),
    0x25BA: ('pointerRight', glyph((triangle('right', LARGE[1], SMALL_SIDE[1]), False))),
    0x25BC: ('triDown', glyph((triangle('down', *LARGE), False))),
    0x25BE: ('triDownSmall', glyph((triangle('down', *SMALL_DOWN), False))),
    0x25C0: ('triLeft', glyph((triangle('left', LARGE[1], LARGE[0]), False))),
    0x25C2: ('triLeftSmall', glyph((triangle('left', *SMALL_SIDE), False))),
    0x25C8: ('diamondDot', glyph((diamond(260, 300), False), (diamond(260 - STROKE, 300 - STROKE * 300 / 260), True),
                                 (diamond(110, 127), False))),
}

empty = TTGlyphPen(None).glyph()
order = ['.notdef', 'space'] + [name for name, _ in shapes.values()]
builder = FontBuilder(1000, isTTF=True)
builder.setupGlyphOrder(order)
builder.setupCharacterMap({0x20: 'space', **{code: name for code, (name, _) in shapes.items()}})
builder.setupGlyf({'.notdef': empty, 'space': empty, **{name: g for name, g in shapes.values()}})
builder.setupHorizontalMetrics({name: (ADVANCE, 0) for name in order})
builder.setupHorizontalHeader(ascent=1025, descent=-275)
builder.setupNameTable({'familyName': 'TennoWorth Shapes', 'styleName': 'Regular'})
builder.setupOS2(sTypoAscender=1025, sTypoDescender=-275, sTypoLineGap=0, usWinAscent=1025, usWinDescent=275,
                 sxHeight=516, sCapHeight=698)
builder.setupPost()
builder.font.flavor = 'woff2'
out = Path(__file__).resolve().parent.parent / 'public' / 'fonts' / 'tw-shapes.woff2'
builder.save(out)
print(out, out.stat().st_size, 'bytes')
