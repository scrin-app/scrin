# Passphrase wordlists (D24)

Passphrase quick connect lets a host show about five short words that a person can dictate over
the phone instead of an ID and a code. Each word encodes **10 bits**, so each list has exactly
**1024 words**. A word's index is its value. Index _i_ in `en.txt` and index _i_ in `ro.txt`
encode the same 10 bits, but the two words are unrelated. They are not translations.

The Rust code in `scrin-crypto` embeds these files with `include_str!`. Changing a word, or the
order of words, changes the encoding, so treat both files as part of the wire contract.

## Files

| File | Contents |
|---|---|
| `en.txt` | 1024 English words, lowercase ASCII `a-z`, 3–8 letters |
| `ro.txt` | 1024 Romanian words, lowercase `a-z` + `ă â î ș ț`, 3–9 letters |
| `check.ps1` | Validator for every rule below |

Both lists are UTF-8 without a BOM and NFC-normalised. They have one word per line, LF line
endings and a trailing newline.

## Rules

1. **Exactly 1024 words** per list.
2. **Charset and length.** EN uses `a-z` with 3–8 letters. RO uses `a-z ă â î ș ț` with 3–9
   letters. Romanian `ș` and `ț` must be comma-below (U+0219, U+021B), **never** cedilla
   (`ş` U+015F, `ţ` U+0163).
3. **Easy to dictate.** Use common, concrete, kid-friendly words such as animals, food, colours,
   objects, nature and simple verbs. Exclude offensive, sad, violent, sexual, religious,
   political and brand words.
4. **No homophones** within a list (e.g. not both `sea` and `see`), and no singular/plural
   pairs.
5. **Diacritic folding.** Folding maps `ă→a`, `â→a`, `î→i`, `ș→s` and `ț→t`. After folding,
   every word is unique within and across both lists. A user who types without diacritics or
   mixes languages always gets one meaning.
6. **4-letter prefixes.** The first 4 folded letters identify a word, within and across both
   lists. A word shorter than 4 letters must not be the start of another word's prefix. For
   example, `cal` blocks `calm`.

## Validate

```powershell
pwsh -NoProfile -File crates/scrin-crypto/wordlists/check.ps1
```

The validator exits `0` and prints `wordlists OK: …` when every rule holds. Otherwise it prints
one `FAIL` line per violation and exits `1`. Rule 3 and rule 4 need human judgement, so review
them when you change a list.

## Sources and licence

The scrin contributors wrote these lists for this project. They are original, not derived from
BIP-39, EFF or any other published wordlist. They are licensed **AGPL-3.0-only**, like the rest
of the repository.
