#!/usr/bin/env python3
"""Builds the fonts the app carries from their OFL sources, so they can be built again.

Inter comes from the variable Latin web subset of rsms/inter, cut to three weights, and its digits
are mapped to the tabular ones so amounts line up in a ledger without the toolkit's OpenType help.
Noto Sans KR is the Korean face of the variable Noto Sans CJK, cut to the same weights: Regular
keeps every Hangul syllable so any name can show, the others keep the common 2,350. Noto Emoji is cut
to the few the developer's note uses.

    python3 make-fonts.py INTER.woff2 NOTO-CJK.ttc NOTO-EMOJI.ttf
"""
import sys
from fontTools.subset import Options, Subsetter
from fontTools.ttLib import TTCollection, TTFont
from fontTools.varLib import instancer

WEIGHTS = {"Regular": 400, "Medium": 500, "SemiBold": 600}
TABULAR = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"]
TABULAR_MARKS = {"comma": 0x2C, "period": 0x2E, "colon": 0x3A}


def plain_options():
    options = Options()
    options.layout_features = ["ccmp", "locl"]
    options.hinting = False
    options.notdef_outline = True
    options.name_IDs = [1, 2, 4, 6, 13, 14]
    options.drop_tables += ["DSIG", "MVAR", "STAT", "meta"]
    return options


def tabular(font):
    cmap = font.getBestCmap()
    for table in font["cmap"].tables:
        if not table.isUnicode():
            continue
        for offset, name in enumerate(TABULAR):
            table.cmap[0x30 + offset] = f"{name}.tf"
        for name, code in TABULAR_MARKS.items():
            table.cmap[code] = f"{name}.tf"
    return cmap


def inter(source):
    for name, weight in WEIGHTS.items():
        font = TTFont(source)
        instance = instancer.instantiateVariableFont(font, {"wght": weight})
        tabular(instance)
        subsetter = Subsetter(plain_options())
        subsetter.populate(unicodes=list(instance.getBestCmap().keys()))
        subsetter.subset(instance)
        instance.flavor = None
        instance.save(f"Inter-{name}.ttf")


def ks_x_1001():
    syllables = set()
    for lead in range(0xB0, 0xC9):
        for trail in range(0xA1, 0xFF):
            try:
                syllables.add(ord(bytes([lead, trail]).decode("euc-kr")))
            except UnicodeDecodeError:
                pass
    return syllables


def noto(source):
    punctuation = set(range(0x3000, 0x3040)) | set(range(0x3131, 0x3190)) | {0x2026, 0x00B7, 0x203B}
    for name, weight in WEIGHTS.items():
        font = TTCollection(source).fonts[1]
        instance = instancer.instantiateVariableFont(font, {"wght": weight})
        wanted = set(range(0xAC00, 0xD7A4)) if name == "Regular" else ks_x_1001()
        subsetter = Subsetter(plain_options())
        subsetter.populate(unicodes=sorted(wanted | punctuation))
        subsetter.subset(instance)
        instance.save(f"NotoSansKR-{name}.otf")


def emoji(path):
    font = TTFont(path)
    subsetter = Subsetter(plain_options())
    subsetter.populate(unicodes=[0x1F605, 0x1F9E1, 0x1F645, 0x1F310, 0x1F913])
    subsetter.subset(font)
    font.save("NotoEmoji-Subset.ttf")


if __name__ == "__main__":
    inter(sys.argv[1])
    noto(sys.argv[2])
    emoji(sys.argv[3])
