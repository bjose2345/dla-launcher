#!/usr/bin/env python3
"""Builds the launcher's list of Japanese person names from JMnedict.

JMnedict (https://www.edrdg.org/enamdict/enamdict_doc.html) is the property
of the Electronic Dictionary Research and Development Group and is used under
its licence, CC BY-SA 4.0 (https://www.edrdg.org/edrdg/licence.html). The
list built here is a trimmed version of it under the same licence.

Usage: build-name-list.py JMnedict.xml.gz crates/dla-translator/data/names.tsv.gz

Each line of the output is `name<TAB>kinds<TAB>spelling|spelling…`, sorted by
name so the launcher can search it without an index:

- kinds: s for a surname, g for a given name, u for a name of either kind,
  and f for a name written in katakana, usually from another language;
- spellings: for a Japanese name, its readings written out in romaji
  (long vowels doubled, no hyphens: Onozuka, Yuuka, Ijuuin); for a katakana
  name, the English forms JMnedict gives (Marsha, Siegfried), in its order.
  JMnedict does not say which reading of a name is common, so a Japanese
  name's readings go in this order: first a reading whose entry is of more
  kinds (a surname that is also a place name), then one more names share
  (はせがわ is the reading of dozens of names, させがわ of one). Of 50 common
  names, 43 come out with their usual reading first and 48 within the four
  kept.
"""

import gzip
import io
import sys
import xml.etree.ElementTree as ET

PERSON = {
    "family or surname": "s",
    "female given name or forename": "g",
    "male given name or forename": "g",
    "given name or forename, gender not specified": "g",
    "unclassified name": "u",
}
MOST_SPELLINGS = 4

DIGRAPHS = {
    "きゃ": "kya", "きゅ": "kyu", "きょ": "kyo", "しゃ": "sha", "しゅ": "shu", "しょ": "sho",
    "ちゃ": "cha", "ちゅ": "chu", "ちょ": "cho", "にゃ": "nya", "にゅ": "nyu", "にょ": "nyo",
    "ひゃ": "hya", "ひゅ": "hyu", "ひょ": "hyo", "みゃ": "mya", "みゅ": "myu", "みょ": "myo",
    "りゃ": "rya", "りゅ": "ryu", "りょ": "ryo", "ぎゃ": "gya", "ぎゅ": "gyu", "ぎょ": "gyo",
    "じゃ": "ja", "じゅ": "ju", "じょ": "jo", "ぢゃ": "ja", "ぢゅ": "ju", "ぢょ": "jo",
    "びゃ": "bya", "びゅ": "byu", "びょ": "byo", "ぴゃ": "pya", "ぴゅ": "pyu", "ぴょ": "pyo",
}
MONOGRAPHS = dict(zip(
    "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわゐゑをん"
    "がぎぐげござじずぜぞだぢづでどばびぶべぼぱぴぷぺぽゔぁぃぅぇぉゃゅょゎ",
    "a i u e o ka ki ku ke ko sa shi su se so ta chi tsu te to na ni nu ne no ha hi fu he ho "
    "ma mi mu me mo ya yu yo ra ri ru re ro wa i e o n ga gi gu ge go za ji zu ze zo da ji zu de do "
    "ba bi bu be bo pa pi pu pe po vu a i u e o ya yu yo wa".split(),
))


def hiragana(text):
    return "".join(chr(ord(c) - 0x60) if "ァ" <= c <= "ヶ" else c for c in text)


def romaji(reading):
    """Hepburn without macrons: long vowels as written, っ doubles the next consonant."""
    kana = hiragana(reading)
    out = []
    double = False
    i = 0
    while i < len(kana):
        pair = kana[i:i + 2]
        if pair in DIGRAPHS:
            syllable, i = DIGRAPHS[pair], i + 2
        elif kana[i] == "っ":
            double, i = True, i + 1
            continue
        elif kana[i] == "ー":
            syllable, i = (out[-1][-1] if out else ""), i + 1
        elif kana[i] in MONOGRAPHS:
            syllable, i = MONOGRAPHS[kana[i]], i + 1
        else:
            return None
        if double:
            syllable = ("t" + syllable) if syllable.startswith("ch") else syllable[0] + syllable
            double = False
        out.append(syllable)
    text = "".join(out)
    return text[:1].upper() + text[1:] if text else None


def is_katakana(text):
    return all("ァ" <= c <= "ヺ" or c in "ー・" for c in text)


def main(source, target):
    # name -> {"kinds": set, "spellings": {spelling: (reading, kinds of its entry, order)}}
    names = {}
    # reading -> the names written with it
    shared = {}
    order = 0
    with gzip.open(source) as stream:
        for _, element in ET.iterparse(stream, events=("end",)):
            if element.tag != "entry":
                continue
            kebs = [k.text for k in element.iter("keb")]
            rebs = [r.text for r in element.iter("reb")]
            if any(t.text in PERSON for t in element.iter("name_type")):
                for reading in rebs:
                    shared.setdefault(reading, set()).update(kebs)
            for trans in element.iter("trans"):
                types = [t.text for t in trans.iter("name_type")]
                kinds = {PERSON[t] for t in types if t in PERSON}
                if not kinds:
                    continue
                breadth = len(types)
                details = [d.text for d in trans.iter("trans_det") if d.text]
                for name in kebs + rebs:
                    if not name:
                        continue
                    if is_katakana(name):
                        spellings = [(None, detail) for detail in details]
                        name_kinds = {"f"}
                    elif name in rebs:
                        spellings = [(name, romaji(name))]
                        name_kinds = kinds
                    else:
                        spellings = [(r, romaji(r)) for r in rebs if not is_katakana(r)]
                        name_kinds = kinds
                    spellings = [(reading, spelling) for reading, spelling in spellings if spelling]
                    if not spellings:
                        continue
                    record = names.setdefault(name, {"kinds": set(), "spellings": {}})
                    record["kinds"] |= name_kinds
                    for reading, spelling in spellings:
                        order += 1
                        known = record["spellings"].get(spelling)
                        if known is None or breadth > known[1]:
                            record["spellings"][spelling] = (reading, breadth, known[2] if known else order)
            element.clear()

    lines = []
    for name in sorted(names):
        record = names[name]
        def likelier(spelling):
            reading, breadth, seen = record["spellings"][spelling]
            return (-breadth, -len(shared.get(reading, ())), seen)

        spellings = sorted(record["spellings"], key=likelier)
        kinds = "".join(k for k in "sguf" if k in record["kinds"])
        lines.append(f"{name}\t{kinds}\t{'|'.join(spellings[:MOST_SPELLINGS])}\n")
    data = "".join(lines).encode("utf-8")
    # The same input always gives the same bytes: no time or file name inside.
    buffer = io.BytesIO()
    with gzip.GzipFile(fileobj=buffer, mode="wb", compresslevel=9, mtime=0, filename="") as out:
        out.write(data)
    with open(target, "wb") as out:
        out.write(buffer.getvalue())
    print(f"{len(lines)} names, {len(data)} bytes, {len(buffer.getvalue())} compressed")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
