# Flip verification worksheet (arm B vs arm A)

A fixed flip only counts if the gold answer was in the store **and** in a frame Terra actually saw. Grounding is date-format-aware (August 16, 2008 == 2008-08-16). The checkboxes are pre-ticked from that automated grounding; the RapidFuzz score is an advisory for anything ambiguous — confirm those by hand.

## FIXED (16)

### qid=16 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** What player scored all the conversions for Spain in the rugby match between Spain and Romania that was part of the 2022 Rugby Europe Championship on February 27, 2022?
- **gold:** `Manuel Ordas`
- **arm A answer:** 'Jaime Manteca'
- **arm B answer:** 'Manuel Ordás scored all of Spain’s conversions.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 5ab7c437.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Spain Romania",
        "2022 Rugby Europe Championship",
        "February 27 2022",
        "conversions"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[0]",
          "submitted": "Spain Romania",
          "resolved": false,
          "candidates": [
            {
              "ref": "#71",
              "name": "Mihai Macovei",
              "kind": "person",
              "score": 0.73
            },
            {
              "ref": "#78",
              "name": "Romania–Russia match (5 February 2022)",
              "kind": "sports match",
              "score": 0.73
            },
            {
              "ref": "#84",
              "name": "Romania–Portugal match (12 February 2022)",
              "kind": "sports match",
              "score": 0.73
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0.64
            },
            {
              "ref": "#70",
              "name": "Andy Robinson",
              "kind": "person",
              "score": 0.64
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0.55
            },
            {
              "ref": "#12581",
              "name": "Pierre-Henri de Valenciennes",
              "kind": "person",
              "score": 0.55
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0.45
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#69",
              "name": "Tomás Appleton",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#72",
              "name": "Dick Muir",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#73",
              "name": "Victor Gresev",
              "kind": "person",
              "score": 0
            
```
</details>

### qid=531 — FIXED  (A=incorrect → B=correct)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** How many million viewers of the inaugural season of Ultimate Kho Kho (UKK) were from India?
- **gold:** `41 million`
- **arm A answer:** '164 million'
- **arm B answer:** 'The inaugural Ultimate Kho Kho season drew **41 million viewers in India**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **False** · on pages: 44dba792.md=True, 139e6e92.md=True  · fuzz(gold, frames): 50

  - [x] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Ultimate Kho Kho",
        "inaugural season viewers India"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "inaugural season viewers India",
          "resolved": false,
          "candidates": [
            {
              "ref": "#2189",
              "name": "2022 Ultimate Kho Kho",
              "kind": "sports season",
              "score": 0.54
            },
            {
              "ref": "#2284",
              "name": "2022 Ultimate Kho Kho exhibition match",
              "kind": "sports event",
              "score": 0.54
            },
            {
              "ref": "#2286",
              "name": "2022 Ultimate Kho Kho player draft",
              "kind": "sports draft",
              "score": 0.5
            },
            {
              "ref": "#2300",
              "name": "India non-cricket viewership rank",
              "score": 0.36
            },
            {
              "ref": "#2343",
              "name": "2022 Ultimate Kho Kho Match 17",
              "kind": "sports match",
              "score": 0.36
            },
            {
              "ref": "#7636",
              "name": "first inaugural mention by a U.S. president",
              "score": 0.36
            },
            {
              "ref": "#9032",
              "name": "Floor Is Lava",
              "kind": "television series",
              "score": 0.36
            },
            {
              "ref": "#2227",
              "name": "India viewership",
              "score": 0.32
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
            {
              "
```
</details>

### qid=796 — FIXED  (A=incorrect → B=correct)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** What were the day, month, and year when Google released a version of Chrome on Windows that added hardware-accelerated H.264 video decoding?
- **gold:** `6 November 2012`
- **arm A answer:** '8 February 2012'
- **arm B answer:** 'Google released **Chrome 23** for Windows on **6 November 2012**. It added hardware-accelerated H.264 video decoding.'

- grounding (date-aware) — in store: **True** · in frames B saw: **False** · on pages: 217cd58e.md=True  · fuzz(gold, frames): 60

  - [x] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Google Chrome",
        "Windows hardware-accelerated H.264 decoding"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "Windows hardware-accelerated H.264 decoding",
          "resolved": false,
          "candidates": [
            {
              "ref": "#2730",
              "name": "Windows H.264 decoding added",
              "score": 0.49
            },
            {
              "ref": "#2732",
              "name": "Windows H.264 decoding",
              "score": 0.49
            },
            {
              "ref": "#2733",
              "name": "hardware-accelerated",
              "score": 0.44
            },
            {
              "ref": "#3166",
              "name": "supports hardware acceleration",
              "score": 0.44
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#69",
              "name": "Tomás Appleton",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#70",
              "name": "Andy Robinson",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#71",
              "name": "Mihai Macovei",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#72",
              "name": "Dick Muir",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#73",
              "name": "Victor Gresev",
              "kind": "person",
              "score": 0
            },
            {
     
```
</details>

### qid=954 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** When (month-day-year) was "Amazing Love" by the Mississippi Mass Choir released?
- **gold:** `June 4, 2002`
- **arm A answer:** '01-01-1995'
- **arm B answer:** '“Amazing Love” by the Mississippi Mass Choir was released on **06-04-2002**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 9118d10c.md=True  · fuzz(gold, frames): 50

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Amazing Love",
        "Mississippi Mass Choir",
        "release date"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#4339",
          "name": "Amazing Love",
          "kind": "album",
          "summary": "Album by the Mississippi Mass Choir."
        },
        {
          "ref": "#4258",
          "name": "Mississippi Mass Choir",
          "kind": "organization",
          "summary": "American gospel choir based in Jackson, Mississippi."
        },
        {
          "ref": "#3178",
          "name": "release date"
        }
      ],
      "recent": [
        {
          "ref": "rel:9421",
          "attrs": {
            "subject": "Amazing Love",
            "release date": "2002-06-04"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9259",
          "attrs": {
            "subject": "Mosie Burks",
            "member of": "Mississippi Mass Choir"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9258",
          "attrs": {
            "subject": "Mississippi Mass Choir",
            "eleventh live album recorded at": "Word of Life Church (Flowood, Mississippi)"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9257",
          "attrs": {
            "subject": "Mississippi Mass Choir",
            "eleventh live album recorded on": "2022-10-28"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9250",
          "attrs": {
            "subject": "Declaration of Dependence",
            "artist": "Mississippi Mass Choir"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  }
]
```
</details>

### qid=1152 — FIXED  (A=not_attempted → B=correct)  — ✓ grounded

- **Q:** What was the date, month, and year when Clifford Cunnell, an English cricketer, died?
- **gold:** `5 October 2016`
- **arm A answer:** "I don't know."
- **arm B answer:** 'Clifford Cunnell died on **5 October 2016**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 8b3def4d.md=True  · fuzz(gold, frames): 50

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Clifford Cunnell",
        "death date"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#4442",
          "name": "Clifford James Cunnell",
          "kind": "person",
          "summary": "English cricketer who played for Suffolk."
        },
        {
          "ref": "#4736",
          "name": "death date"
        }
      ],
      "recent": [
        {
          "ref": "rel:9697",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "brother": "Bob Cunnell"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9696",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "List A opponent": "Kent County Cricket Club"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9695",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "List A competition": "1966 Gillette Cup"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9694",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "List A appearances": "1"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9693",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "last Suffolk season": "1972"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  },
  {
    "args": {
      "focus": [
        "Clifford James Cunnell date of death died",
        "Clifford Cunnell biography"
      ],
      "limit": 20,
      "history_depth": 2
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[0]",
          "submitted": "Clifford James Cunnell date of death died",
          "resolved": false,
          "candidates": [
            {
              "ref": "#4442",
              "name": "Clifford James Cunnell",
              "kind": "person",
              "score": 0.51
            },
            {
              "ref": "#4443",
              "name": "Bob Cunnell",
              "kind": "person",
              "score": 0.41
            },
            {
              "ref": "#4447",
              "name": "Minor Counties Championship",
              "kind": "competition",
              "score": 0.36
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadiu
```
</details>

### qid=1415 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** What was the Hampstead, London, street address of art collector Margaret Gardiner's home?
- **gold:** `35 Downshire Hill`
- **arm A answer:** '3 Willow Road, Hampstead, London.'
- **arm B answer:** 'Margaret Gardiner’s Hampstead home was at **35 Downshire Hill, London**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 5152620f.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Margaret Gardiner",
        "Hampstead home address"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "Hampstead home address",
          "resolved": false,
          "candidates": [
            {
              "ref": "#4660",
              "name": "35 Downshire Hill",
              "kind": "place",
              "score": 0.55
            },
            {
              "ref": "#4693",
              "name": "Hampstead residence start year",
              "score": 0.45
            },
            {
              "ref": "#7500",
              "name": "White House Task Force to Protect Students from Sexual Assault",
              "kind": "organization",
              "score": 0.45
            },
            {
              "ref": "#12637",
              "name": "Hampstead Heath with a Rainbow",
              "kind": "artwork",
              "score": 0.45
            },
            {
              "ref": "#2701",
              "name": "Omnibox",
              "kind": "software feature",
              "score": 0.4
            },
            {
              "ref": "#3301",
              "name": "Chrome Omnibox suggestion service",
              "kind": "service",
              "score": 0.4
            },
            {
              "ref": "#3307",
              "name": "IP Protection",
              "kind": "technology",
              "score": 0.4
            },
            {
              "ref": "#4661",
              "name": "Hampstead",
              "kind": "place",
              "score": 0.4
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
            {
           
```
</details>

### qid=1605 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** Which Chinese president was nicknamed "Mr. Democracy?"
- **gold:** `Lee Teng-hui.`
- **arm A answer:** 'Li Yuanhong.'
- **arm B answer:** 'Lee Teng-hui, the former president of Taiwan (the Republic of China), was nicknamed “Mr. Democracy.”'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 292e0757.md=True  · fuzz(gold, frames): 92

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Chinese president",
        "Mr. Democracy"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[0]",
          "submitted": "Chinese president",
          "resolved": false,
          "candidates": [
            {
              "ref": "#4990",
              "name": "Chao Tzu-chi",
              "kind": "person",
              "score": 0.87
            },
            {
              "ref": "#4709",
              "name": "Lee Teng-hui",
              "kind": "person",
              "score": 0.8
            },
            {
              "ref": "#4713",
              "name": "Chen Shui-bian",
              "kind": "person",
              "score": 0.8
            },
            {
              "ref": "#4718",
              "name": "Lin Yang-kang",
              "kind": "person",
              "score": 0.8
            },
            {
              "ref": "#4909",
              "name": "Peng Ming-min",
              "kind": "person",
              "score": 0.8
            },
            {
              "ref": "#4975",
              "name": "Hau Pei-tsun",
              "kind": "person",
              "score": 0.8
            },
            {
              "ref": "#4979",
              "name": "James Soong",
              "kind": "person",
              "score": 0.8
            },
            {
              "ref": "#4991",
              "name": "Liu Kuo-tsai",
              "kind": "person",
              "score": 0.8
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#69",
              "name": "Tomás Appleton",
              "kind": "person",
              "scor
```
</details>

### qid=1846 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** In what year was the praying mantis species Eremiaphila barbara described by Brisout?
- **gold:** `1854`
- **arm A answer:** '1860'
- **arm B answer:** 'Eremiaphila barbara was described by Brisout in **1854**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 089f12d8.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Eremiaphila barbara",
        "described Brisout"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "described Brisout",
          "resolved": false,
          "candidates": [
            {
              "ref": "#8620",
              "name": "Plan for post-Saddam Iraq",
              "kind": "work",
              "score": 0.67
            },
            {
              "ref": "#885",
              "name": "Shraddhadeva Manu",
              "kind": "deity",
              "score": 0.6
            },
            {
              "ref": "#1003",
              "name": "Vidura",
              "kind": "person",
              "score": 0.6
            },
            {
              "ref": "#9251",
              "name": "described by",
              "score": 0.6
            },
            {
              "ref": "#10386",
              "name": "Carl Gustaf Mannerheim",
              "kind": "person",
              "score": 0.6
            },
            {
              "ref": "#10387",
              "name": "Pierre François Marie Auguste Dejean",
              "kind": "person",
              "score": 0.6
            },
            {
              "ref": "#10388",
              "name": "Gotthelf Fischer von Waldheim",
              "kind": "person",
              "score": 0.6
            },
            {
              "ref": "#10431",
              "name": "described specimens collected by",
              "score": 0.6
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#69",
              "name": "Tomás Appleton",
              "kind": "person
```
</details>

### qid=2082 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** What is the name of the recipient of the Lapworth Medal in 2004?
- **gold:** `James Valentine`
- **arm A answer:** 'Professor John C. W. Cope'
- **arm B answer:** 'James Valentine.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: c21fbe30.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Lapworth Medal",
        "2004 recipient"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#6500",
          "name": "Lapworth Medal",
          "kind": "award",
          "summary": "The highest award of the Palaeontological Association."
        },
        {
          "ref": "#6547",
          "name": "2004 recipient"
        }
      ],
      "recent": [
        {
          "ref": "rel:14727",
          "attrs": {
            "subject": "Lapworth Medal",
            "2000 recipient": "Harry B. Whittington"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:14726",
          "attrs": {
            "subject": "Lapworth Medal",
            "2002 recipient": "Alwyn Williams"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:14725",
          "attrs": {
            "subject": "Lapworth Medal",
            "2004 recipient": "James Valentine"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:14724",
          "attrs": {
            "subject": "Lapworth Medal",
            "2005 recipient": "William Chaloner"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:14723",
          "attrs": {
            "subject": "Lapworth Medal",
            "2006 recipient": "Dolf Seilacher"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  }
]
```
</details>

### qid=2857 — FIXED  (A=incorrect → B=correct)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** In what year was Peter Scholze appointed the Chancellor's Professor at the University of California, Berkeley?
- **gold:** `2014`
- **arm A answer:** '2019'
- **arm B answer:** 'Peter Scholze was appointed a Chancellor’s Professor at the University of California, Berkeley in **2014**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **False** · on pages: 070980a9.md=True  · fuzz(gold, frames): 75

  - [x] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Peter Scholze",
        "Chancellor's Professor",
        "University of California, Berkeley"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "Chancellor's Professor",
          "ref": "#10852",
          "name": "chancellor's professor at",
          "via": "lexical",
          "score": 1
        }
      ],
      "focus": [
        {
          "ref": "#10814",
          "name": "Peter Scholze",
          "kind": "person",
          "summary": "German mathematician known for work in arithmetic geometry."
        },
        {
          "ref": "#10852",
          "name": "chancellor's professor at"
        },
        {
          "ref": "#10819",
          "name": "University of California, Berkeley",
          "kind": "organization",
          "summary": "Public research university in Berkeley, California."
        }
      ],
      "recent": [
        {
          "ref": "rel:24827",
          "attrs": {
            "subject": "Peter Scholze",
            "child count": "1"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24826",
          "attrs": {
            "subject": "Peter Scholze",
            "Pius XI Medal year": "2022"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24825",
          "attrs": {
            "subject": "Peter Scholze",
            "received award": "Pius XI Medal"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24824",
          "attrs": {
            "subject": "Peter Scholze",
            "Royal Society foreign membership year": "2022"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24823",
          "attrs": {
            "subject": "Peter Scholze",
            "foreign member of": "Royal Society"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  }
]
```
</details>

### qid=2919 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** In what year did Gerald M. Friedman receive the William Henry Twenhofel Medal?
- **gold:** `1997`
- **arm A answer:** '2006'
- **arm B answer:** 'Gerald M. Friedman received the William H. Twenhofel Medal in **1997**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: ce41018e.md=True, 708a4916.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Gerald M. Friedman",
        "William Henry Twenhofel Medal"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "William Henry Twenhofel Medal",
          "resolved": false,
          "candidates": [
            {
              "ref": "#10875",
              "name": "William H. Twenhofel",
              "kind": "person",
              "score": 0.81
            },
            {
              "ref": "#10873",
              "name": "William H. Twenhofel Medal",
              "kind": "award",
              "score": 0.78
            },
            {
              "ref": "#10876",
              "name": "Raymond C. Moore",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10877",
              "name": "Francis J. Pettijohn",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10878",
              "name": "Edwin D. McKee",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10879",
              "name": "Robert R. Shrock",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10880",
              "name": "William C. Krumbein",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10881",
              "name": "Carl Owen Dunbar",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#69",

```
</details>

### qid=3007 — FIXED  (A=not_attempted → B=correct)  — ✓ grounded

- **Q:** For how many English guineas did the Victoria and Albert Museum purchase the "Bust of Thomas Baker" by Gian Lorenzo Bernini in 1921?
- **gold:** `1480 English guineas`
- **arm A answer:** "I don't know."
- **arm B answer:** 'The Victoria and Albert Museum purchased it for **1,480 English guineas**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 0f17c028.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Bust of Thomas Baker",
        "Victoria and Albert Museum purchase 1921",
        "English guineas"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "Victoria and Albert Museum purchase 1921",
          "resolved": false,
          "candidates": [
            {
              "ref": "#2431",
              "name": "Victoria and Albert Museum",
              "kind": "organization",
              "score": 0.63
            },
            {
              "ref": "#10616",
              "name": "Museum of Modern Art, Rio de Janeiro",
              "kind": "organization",
              "score": 0.32
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#69",
              "name": "Tomás Appleton",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#70",
              "name": "Andy Robinson",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#71",
              "name": "Mihai Macovei",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#72",
              "name": "Dick Muir",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#73",
              "name": "Victor Gresev",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#74",
              "name": "Santiago Santos",
              "kind": "person",
              "score": 0
            },
            {
         
```
</details>

### qid=3442 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** In which year did George Bentham change the name of *Hedaroma pinifolium* to *Pimelea pinifolia*?
- **gold:** `1865`
- **arm A answer:** '1873'
- **arm B answer:** 'George Bentham changed it to *Pimelea pinifolia* in **1865**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: afb4bcf2.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "George Bentham",
        "Hedaroma pinifolium",
        "Pimelea pinifolia",
        "name change"
      ],
      "limit": 5,
      "history_depth": 2
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[3]",
          "submitted": "name change",
          "resolved": false,
          "candidates": [
            {
              "ref": "#12679",
              "name": "George Bentham",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#12681",
              "name": "Journal of the Linnean Society, Botany",
              "kind": "periodical",
              "score": 1
            },
            {
              "ref": "#12737",
              "name": "name-change publication",
              "score": 1
            },
            {
              "ref": "#8077",
              "name": "Iraqi regime change",
              "kind": "concept",
              "score": 0.78
            },
            {
              "ref": "#8119",
              "name": "mentioned regime change",
              "score": 0.78
            },
            {
              "ref": "#8194",
              "name": "regime change",
              "score": 0.78
            },
            {
              "ref": "#8392",
              "name": "regime-change policy",
              "score": 0.78
            },
            {
              "ref": "#8525",
              "name": "Fixing intelligence around Iraq regime-change policy",
              "kind": "concept",
              "score": 0.78
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#69",
              "name": "Tomás Appleton",
              "kind"
```
</details>

### qid=3754 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** Name the place in Lahore where an underpass (Habba Khatoon Underpass) has been named after Habba Khatoon (a Kashmiri poetess).
- **gold:** `Mughalpura`
- **arm A answer:** 'Kashmir Chowk, Lahore.'
- **arm B answer:** 'The Habba Khatoon Underpass is in **Mughalpura, Lahore**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 93df96cd.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Habba Khatoon Underpass",
        "Lahore"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#13764",
          "name": "Habba Khatoon Underpass",
          "kind": "place",
          "summary": "Underpass in Mughalpura named after Habba Khatoon."
        },
        {
          "ref": "#6677",
          "name": "Lahore",
          "kind": "place",
          "summary": "City in Pakistan."
        }
      ],
      "recent": [
        {
          "ref": "rel:32843",
          "attrs": {
            "subject": "Habba Khatoon Underpass",
            "named after": "Habba Khatoon"
          },
          "status": "asserted",
          "confidence": 0.98,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:32842",
          "attrs": {
            "subject": "Mughalpura",
            "located in": "Lahore"
          },
          "status": "asserted",
          "confidence": 0.98,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:32841",
          "attrs": {
            "subject": "Habba Khatoon Underpass",
            "located in": "Mughalpura"
          },
          "status": "asserted",
          "confidence": 0.98,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:16037",
          "attrs": {
            "subject": "Pakistan Peoples Party Parliamentarians-Patriots",
            "launch place": "Lahore"
          },
          "status": "asserted",
          "confidence": 0.95,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:15000",
          "attrs": {
            "subject": "Pakistan People's Party",
            "founded in": "Lahore"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  }
]
```
</details>

### qid=4222 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** On what day, month, and year was Manuleleua Paletasala Tovale appointed Associate Minister for the Prime Minister and Cabinet?
- **gold:** `28 July 2021`
- **arm A answer:** '28 February 2024'
- **arm B answer:** 'Manuleleua Paletasala Tovale was appointed Associate Minister for the Prime Minister and Cabinet on **28 July 2021**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 6b12f023.md=True  · fuzz(gold, frames): 50

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Manuleleua Paletasala Tovale",
        "appointed Associate Minister Prime Minister Cabinet"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "appointed Associate Minister Prime Minister Cabinet",
          "resolved": false,
          "candidates": [
            {
              "ref": "#14614",
              "name": "Associate Minister for the Prime Minister and Cabinet of Samoa",
              "kind": "political office",
              "score": 0.75
            },
            {
              "ref": "#14629",
              "name": "appointed as associate minister",
              "score": 0.65
            },
            {
              "ref": "#10541",
              "name": "Arvind Kejriwal",
              "kind": "person",
              "score": 0.63
            },
            {
              "ref": "#5087",
              "name": "Junichiro Koizumi",
              "kind": "person",
              "score": 0.6
            },
            {
              "ref": "#14615",
              "name": "Fiamē Naomi Mataʻafa",
              "kind": "person",
              "score": 0.57
            },
            {
              "ref": "#5879",
              "name": "Yusuf Raza Gilani",
              "kind": "person",
              "score": 0.48
            },
            {
              "ref": "#14631",
              "name": "dismissed as associate minister",
              "score": 0.48
            },
            {
              "ref": "#1883",
              "name": "Kimon Georgiev",
              "kind": "person",
              "score": 0.45
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0

```
</details>

### qid=4313 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** Who was the mayor of Kathmandu who served from 1971 to 1976?
- **gold:** `Rajendra Man Suwal`
- **arm A answer:** 'Dhan Bahadur Shakya'
- **arm B answer:** 'Rajendra Man Suwal served as Kathmandu’s **Pradhan Panch (mayor)** from 1971 to 1976.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 394497e6.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Kathmandu mayor",
        "1971 1976"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[0]",
          "submitted": "Kathmandu mayor",
          "resolved": false,
          "candidates": [
            {
              "ref": "#14636",
              "name": "Deputy Mayor of Kathmandu",
              "kind": "public office",
              "score": 1
            },
            {
              "ref": "#14652",
              "name": "Singha Shumsher Jung Bahadur Rana",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#14664",
              "name": "Keshav Sthapit",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#14675",
              "name": "2017 Kathmandu mayoral election",
              "kind": "election",
              "score": 1
            },
            {
              "ref": "#14676",
              "name": "2022 Kathmandu mayoral election",
              "kind": "election",
              "score": 1
            },
            {
              "ref": "#14635",
              "name": "Mayor of Kathmandu",
              "kind": "public office",
              "score": 0.92
            },
            {
              "ref": "#14640",
              "name": "Office of Municipal Executive, Kathmandu",
              "kind": "place",
              "score": 0.92
            },
            {
              "ref": "#14651",
              "name": "Valley Municipal Forum",
              "kind": "organization",
              "score": 0.92
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0
            },
   
```
</details>

## BROKEN (5)

### qid=115 — BROKEN  (A=correct → B=incorrect)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** The Jensen Interceptor (1950), produced from 1950 to 1957, had a wheelbase measurement of what in millimeters?
- **gold:** `2,845 mm`
- **arm A answer:** '2845 mm'
- **arm B answer:** 'The 1950–1957 Jensen Interceptor had a wheelbase of **2,667 mm** (105 inches).'

- grounding (date-aware) — in store: **False** · in frames B saw: **False** · on pages: bb2a02a9.md=True  · fuzz(gold, frames): 50

  - [ ] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Jensen Interceptor (1950)",
        "wheelbase"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#767",
          "name": "Jensen Interceptor (1950)",
          "kind": "automobile model",
          "summary": "First-generation Jensen Interceptor produced from 1950 to 1957."
        },
        {
          "ref": "#805",
          "name": "wheelbase"
        }
      ],
      "recent": [
        {
          "ref": "rel:1619",
          "attrs": {
            "subject": "Austin A40 Sports",
            "resembles": "Jensen Interceptor (1950)"
          },
          "status": "asserted",
          "confidence": 0.9,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:1606",
          "attrs": {
            "subject": "Briggs Cunningham's Jensen Interceptor",
            "model": "Jensen Interceptor (1950)"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:1605",
          "attrs": {
            "subject": "Jensen Interceptor (1950)",
            "1952 overdrive price": "£116"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:1604",
          "attrs": {
            "subject": "Jensen Interceptor (1950)",
            "1952 UK price including tax": "£2,645"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:1603",
          "attrs": {
            "subject": "Jensen Interceptor (1950)",
            "tested fuel consumption": "20.3 mpg imperial"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  }
]
```
</details>

### qid=2190 — BROKEN  (A=correct → B=incorrect)  — ✓ grounded

- **Q:** In Season 2 of the American version of "The Circle," what episode did Terilisha get blocked?
- **gold:** `7`
- **arm A answer:** 'Episode 7.'
- **arm B answer:** 'Terilisha was blocked in **Season 2, Episode 4**, **“Lines Are Drawn.”**'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 9d5b155b.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "The Circle US Season 2",
        "Terilisha blocked",
        "episode"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[0]",
          "submitted": "The Circle US Season 2",
          "resolved": false,
          "candidates": [
            {
              "ref": "#9028",
              "name": "Trevor St. Agathe",
              "kind": "person",
              "score": 0.9
            },
            {
              "ref": "#9014",
              "name": "The Circle (American TV series) season 2",
              "kind": "television season",
              "score": 0.85
            },
            {
              "ref": "#9079",
              "name": "Back and More Savage Than Ever",
              "kind": "television episode",
              "score": 0.85
            },
            {
              "ref": "#9081",
              "name": "Alliances Are Formed",
              "kind": "television episode",
              "score": 0.85
            },
            {
              "ref": "#9082",
              "name": "Bye, Bye, Bye!",
              "kind": "television episode",
              "score": 0.85
            },
            {
              "ref": "#9083",
              "name": "Lines Are Drawn",
              "kind": "television episode",
              "score": 0.85
            },
            {
              "ref": "#9084",
              "name": "Snake in the Grass",
              "kind": "television episode",
              "score": 0.85
            },
            {
              "ref": "#9085",
              "name": "A Love Triangle",
              "kind": "television episode",
              "score": 0.85
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
             
```
</details>

### qid=2566 — BROKEN  (A=correct → B=incorrect)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** What is the name of the person who explored and named Bikini Atoll "Eschscholtz Atoll"?
- **gold:** ` Otto von Kotzebue `
- **arm A answer:** 'Otto von Kotzebue'
- **arm B answer:** 'Johann Friedrich von Eschscholtz.'

- grounding (date-aware) — in store: **True** · in frames B saw: **False** · on pages: 538ea0f0.md=True, 08997356.md=True  · fuzz(gold, frames): 47

  - [x] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Bikini Atoll",
        "Eschscholtz Atoll"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#9676",
          "name": "Bikini Atoll",
          "kind": "place",
          "summary": "Marshall Islands atoll formerly named Eschscholtz Atoll."
        }
      ],
      "recent": [
        {
          "ref": "rel:23606",
          "attrs": {
            "subject": "Bikini Atoll",
            "former namesake": "Johann Friedrich von Eschscholtz"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:23605",
          "attrs": {
            "subject": "Bikini Atoll",
            "renamed year": "1946"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:23378",
          "attrs": {
            "subject": "Bikini swimsuit",
            "named after": "Bikini Atoll"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:23375",
          "attrs": {
            "subject": "Able (Operation Crossroads)",
            "detonated over": "Bikini Atoll"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:23370",
          "attrs": {
            "subject": "Bikini Bottom",
            "fictionally situated beneath": "Bikini Atoll"
          },
          "status": "asserted",
          "confidence": 0.95,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  }
]
```
</details>

### qid=2790 — BROKEN  (A=correct → B=incorrect)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** What did the artist Lygia Pape initially study in university?
- **gold:** `Philosophy`
- **arm A answer:** 'Philosophy.'
- **arm B answer:** 'Lygia Pape initially studied **sculpture** at the School of Fine Arts of the University of Brazil in Rio de Janeiro.'

- grounding (date-aware) — in store: **True** · in frames B saw: **False** · on pages: a537d953.md=True  · fuzz(gold, frames): 50

  - [x] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Lygia Pape",
        "university study"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "university study",
          "resolved": false,
          "candidates": [
            {
              "ref": "#3528",
              "name": "Slade School of Fine Art",
              "kind": "organization",
              "score": 0.86
            },
            {
              "ref": "#4355",
              "name": "Trinity College, Oxford",
              "kind": "organization",
              "score": 0.86
            },
            {
              "ref": "#4356",
              "name": "University of Oxford",
              "kind": "organization",
              "score": 0.86
            },
            {
              "ref": "#6005",
              "name": "Budapest University of Technology and Economics",
              "kind": "organization",
              "score": 0.86
            },
            {
              "ref": "#10375",
              "name": "Imperial University of Dorpat",
              "kind": "organization",
              "score": 0.86
            },
            {
              "ref": "#10614",
              "name": "Federal University of Rio de Janeiro",
              "kind": "organization",
              "score": 0.86
            },
            {
              "ref": "#12177",
              "name": "Massachusetts Institute of Technology",
              "kind": "organization",
              "score": 0.86
            },
            {
              "ref": "#4802",
              "name": "Taihoku Higher School",
              "kind": "organization",
              "score": 0.79
            },
            {
              "ref": "#42",
              "name": "2022 Rugby Europe Championship",
              "kind": "sports tournament",
              "score": 0
            },
            {
              "ref": "#43",
              "name": "sports tournament",
              "score": 0
            },
            {
              "ref": "#44",
              "name": "Georgia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#45",
              "name": "sports team",
              "score": 0
            },
            {
              "ref": "#46",
              "name": "Netherlands national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#47",
              "name": "Portugal national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#48",
              "name": "Romania national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#49",
              "name": "Russia national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#50",
              "name": "Spain national rugby union team",
              "kind": "sports team",
              "score": 0
            },
            {
              "ref": "#51",
              "name": "Antim Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#52",
              "name": "sports trophy",
              "score": 0
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0
            },
            {
              "ref": "#54",
              "name": "2023 Rugby World Cup qualification",
              "kind": "sports qualification",
              "score": 0
            },
            {
              "ref": "#55",
              "name": "sports qualification",
              "score": 0
            },
            {
              "ref": "#56",
              "name": "Boris Paichadze Dinamo Arena",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#57",
              "name": "stadium",
              "score": 0
            },
            {
              "ref": "#58",
              "name": "NRCA Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#59",
              "name": "Campo de Rugby do Jamor",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#60",
              "name": "Arcul de Triumf Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#61",
              "name": "Slava Metreveli Central Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#62",
              "name": "Estadio Nacional Complutense",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0
            },
            {
              "ref": "#64",
              "name": "Levan Maisashvili",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#65",
              "name": "Merab Sharikadze",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#66",
              "name": "Zane Gardiner",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#67",
              "name": "Bart Wierenga",
              "kind": "person",
              "score": 0
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              
```
</details>

### qid=3884 — BROKEN  (A=correct → B=incorrect)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** What was the name of the first principal of Tyndale Biscoe School in Srinagar, Kashmir?
- **gold:** `Reverend J.H.Knowles`
- **arm A answer:** 'Rev. J. H. Knowles'
- **arm B answer:** 'The first principal was **Rev. Cecil Tyndale-Biscoe**, who took charge of the school in Srinagar in 1890.'

- grounding (date-aware) — in store: **False** · in frames B saw: **False** · on pages: cce2ed4e.md=False  · fuzz(gold, frames): 35

  - [ ] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [ ] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Tyndale Biscoe School",
        "first principal"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#13803",
          "name": "Tyndale Biscoe School",
          "kind": "school",
          "summary": "Private boys' school in Srinagar, Jammu and Kashmir, founded in 1880."
        },
        {
          "ref": "#13849",
          "name": "first principal"
        }
      ],
      "recent": [
        {
          "ref": "rel:33049",
          "attrs": {
            "subject": "Ashfaq Majeed Wani",
            "alumnus of": "Tyndale Biscoe School"
          },
          "status": "asserted",
          "confidence": 0.98,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:33048",
          "attrs": {
            "subject": "Tanvir Sadiq",
            "alumnus of": "Tyndale Biscoe School"
          },
          "status": "asserted",
          "confidence": 0.95,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:33047",
          "attrs": {
            "subject": "S. L. Sadhu",
            "alumnus of": "Tyndale Biscoe School"
          },
          "status": "asserted",
          "confidence": 0.98,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:33046",
          "attrs": {
            "subject": "Mohammad Shafi Qureshi",
            "alumnus of": "Tyndale Biscoe School"
          },
          "status": "asserted",
          "confidence": 0.98,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:33045",
          "attrs": {
            "subject": "Bakshi Ghulam Mohammad",
            "alumnus of": "Tyndale Biscoe School"
          },
          "status": "asserted",
          "confidence": 0.98,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  }
]
```
</details>
