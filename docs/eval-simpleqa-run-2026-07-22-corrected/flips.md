# Flip verification worksheet (arm B vs arm A)

A fixed flip only counts if the gold answer was in the store **and** in a frame Terra actually saw. Grounding is date-format-aware (August 16, 2008 == 2008-08-16). The checkboxes are pre-ticked from that automated grounding; the RapidFuzz score is an advisory for anything ambiguous — confirm those by hand.

## FIXED (25)

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
        "Spain Romania February 27 2022",
        "Spain conversions"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[0]",
          "submitted": "Spain Romania February 27 2022",
          "resolved": false,
          "candidates": [
            {
              "ref": "#78",
              "name": "Romania–Russia match (5 February 2022)",
              "kind": "sports match",
              "score": 0.64
            },
            {
              "ref": "#84",
              "name": "Romania–Portugal match (12 February 2022)",
              "kind": "sports match",
              "score": 0.61
            },
            {
              "ref": "#82",
              "name": "Russia–Spain match (12 February 2022)",
              "kind": "sports match",
              "score": 0.57
            },
            {
              "ref": "#80",
              "name": "Spain–Netherlands match (5 February 2022)",
              "kind": "sports match",
              "score": 0.54
            },
            {
              "ref": "#63",
              "name": "Mikheil Meskhi Stadium",
              "kind": "stadium",
              "score": 0.5
            },
            {
              "ref": "#83",
              "name": "Netherlands–Georgia match (12 February 2022)",
              "kind": "sports match",
              "score": 0.46
            },
            {
              "ref": "#71",
              "name": "Mihai Macovei",
              "kind": "person",
              "score": 0.43
            },
            {
              "ref": "#81",
              "name": "Georgia–Portugal match (6 February 2022)",
              "kind": "sports match",
              "score": 0.43
            },
            {
              "ref": "#86",
              "name": "5 February 2022",
              "score": 0.73
            },
            {
              "ref": "#88",
              "name": "20 March 2022",
              "score": 0.72
            },
            {
              "ref": "#557",
              "name": "2022",
              "score": 0.72
            },
            {
              "ref": "#10579",
              "name": "February 2020",
              "score": 0.71
            },
            {
              "ref": "#74",
              "name": "Santiago Santos",
              "kind": "person",
              "score": 0.71
            },
            {
              "ref": "#70",
              "name": "Andy Robinson",
              "kind": "person",
              "score": 0.71
            },
            {
              "ref": "#3254",
              "name": "August 2022",
              "score": 0.7
            },
            {
              "ref": "#75",
              "name": "Fernando López",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#4737",
              "name": "30 July 2020",
              "score": 0.69
            },
            {
              "ref": "#10555",
              "name": "23 December 2024",
              "score": 0.68
            },
            {
              "ref": "#76",
              "name": "Manuel Ordas",
              "kind": "person",
              "score": 0.67
            },
            {
              "ref": "#68",
              "name": "Patrice Lagisquet",
              "kind": "person",
              "score": 0.66
            },
            {
              "ref": "#762",
              "name": "2023",
              "score": 0.66
            },
            {
              "ref": "#9041",
              "name": "March 2020",
              "score": 0.66
            },
            {
              "ref": "#53",
              "name": "Kiseleff Cup",
              "kind": "sports trophy",
              "score": 0.66
            },
            {
              "ref": "#3279",
              "name": "December 2023",
              "score": 0.66
            },
            {
              "ref": "#6297",
              "name": "2023 participation",
              "score": 0.66
            },
            {
              "ref": "#3270",
              "name": "August 2025",
              "score": 0.66
            },
            {
              "ref": "#3422",
              "name": "August 2024",
              "score": 0.66
            },
            {
              "ref": "#10564",
              "name": "14 February 2015",
              "score": 0.66
            },
            {
              "ref": "#1506",
              "name": "April 2024",
              "score": 0.66
            },
            {
              "ref": "#736",
              "name": "2024",
              "score": 0.65
            }
          ]
        },
        {
          "at": "focus[1]",
          "submitted": "Spain conversions",
          "resolved": false,
          "candidates": [
            {
              "ref": "#12459",
              "name": "Francis Slater Rebow",
              "kind": "person",
              "score": 0.6
            },
            {
              "ref": "#12519",
              "name": "Foster version of The Lock",
              "kind": "work",
              "score": 0.6
            },
            {
              "ref": "#13904",
              "name": "Mordecai Ham",
              "kind": "person",
              "score": 0.6
            },
            {
              "ref": "#26",
              "name": "pointer",
              "score": 0.53
            },
            {
              "ref": "#31",
              "name": "commit",
              "score": 0.53
            },
            {
              "ref": "#9540",
              "name": "Nye County",
              "kind": "place",
              "score": 0.53
            },
            {
              "ref": "#12448",
              "name": "John Thomas Smith",
              "kind": "person",
              "score": 0.53
            },
            {
              "ref": "#12520",
            
```
</details>

### qid=34 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** On what day, month, and year was David Sweet, Canadian politician, born?
- **gold:** `June 24, 1957`
- **arm A answer:** '15 June 1957'
- **arm B answer:** 'David Sweet was born on 24 June 1957.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 3bb3a2cc.md=True  · fuzz(gold, frames): 38

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "David Sweet",
        "birth date"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#145",
          "name": "David Sweet",
          "kind": "person",
          "summary": "Canadian politician and former member of Parliament."
        },
        {
          "ref": "#4733",
          "name": "birth date"
        }
      ],
      "recent": [
        {
          "ref": "rel:237",
          "attrs": {
            "subject": "David Sweet",
            "date of birth": "1957-06-24"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:239",
          "attrs": {
            "subject": "David Sweet",
            "nationality": "Canadian"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:238",
          "attrs": {
            "subject": "David Sweet",
            "place of birth": "Kingston, Ontario"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:252",
          "attrs": {
            "subject": "David Sweet",
            "age when confinement ended": "16"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:251",
          "attrs": {
            "subject": "David Sweet",
            "age when confinement began": "13"
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

### qid=51 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** In what year was John Williams inducted into the Classical Music Hall of Fame?
- **gold:** `2004.`
- **arm A answer:** '2003'
- **arm B answer:** 'John Williams was inducted into the American Classical Music Hall of Fame in **2004**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 5e77d472.md=True, 9d81d326.md=True  · fuzz(gold, frames): 80

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "John Williams",
        "Classical Music Hall of Fame"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "Classical Music Hall of Fame",
          "resolved": false,
          "candidates": [
            {
              "ref": "#431",
              "name": "American Classical Music Hall of Fame and Museum",
              "kind": "organization",
              "score": 1
            },
            {
              "ref": "#432",
              "name": "Nina Perlove",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#433",
              "name": "David A. Klingshirm",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#446",
              "name": "Marian Anderson",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#447",
              "name": "Samuel Barber",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#448",
              "name": "Leonard Bernstein",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#449",
              "name": "Elliott Carter",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#450",
              "name": "Aaron Copland",
              "kind": "person",
              "score": 1
            },
            {
              "ref": "#495",
              "name": "American Classical Music Hall of Fame induction year",
              "score": 0.79
            },
            {
              "ref": "#476",
              "name": "classical music museum",
              "score": 0.77
            },
            {
              "ref": "#452",
              "name": "George Gershwin",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#451",
              "name": "Duke Ellington",
              "kind": "person",
              "score": 0.76
            },
            {
              "ref": "#456",
              "name": "Serge Koussevitzky",
              "kind": "person",
              "score": 0.76
            },
            {
              "ref": "#466",
              "name": "Isaac Stern",
              "kind": "person",
              "score": 0.75
            },
            {
              "ref": "#470",
              "name": "Arturo Toscanini",
              "kind": "person",
              "score": 0.75
            },
            {
              "ref": "#467",
              "name": "Leopold Stokowski",
              "kind": "person",
              "score": 0.75
            },
            {
              "ref": "#453",
              "name": "Howard Hanson",
              "kind": "person",
              "score": 0.74
            },
            {
              "ref": "#460",
              "name": "Arnold Schoenberg",
              "kind": "person",
              "score": 0.74
            },
            {
              "ref": "#457",
              "name": "John Knowles Paine",
              "kind": "person",
              "score": 0.74
            },
            {
              "ref": "#461",
              "name": "Gunther Schuller",
              "kind": "person",
              "score": 0.74
            },
            {
              "ref": "#459",
              "name": "Fritz Reiner",
              "kind": "person",
              "score": 0.74
            },
            {
              "ref": "#462",
              "name": "Roger Sessions",
              "kind": "person",
              "score": 0.74
            },
            {
              "ref": "#454",
              "name": "Charles Ives",
              "kind": "person",
              "score": 0.74
            },
            {
              "ref": "#468",
              "name": "Igor Stravinsky",
              "kind": "person",
              "score": 0.73
            },
            {
              "ref": "#760",
              "name": "Global Awards Best Classical Artist year",
              "score": 0.73
            },
            {
              "ref": "#464",
              "name": "Nicolas Slonimsky",
              "kind": "person",
              "score": 0.73
            },
            {
              "ref": "#465",
              "name": "John Philip Sousa",
              "kind": "person",
              "score": 0.72
            },
            {
              "ref": "#463",
              "name": "Robert Shaw",
              "kind": "person",
              "score": 0.72
            },
            {
              "ref": "#458",
              "name": "Leontyne Price",
              "kind": "person",
              "score": 0.72
            },
            {
              "ref": "#469",
              "name": "Theodore Thomas",
              "kind": "person",
              "score": 0.71
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#496",
          "name": "John Williams",
          "kind": "person",
          "summary": "American composer known for film and television music."
        }
      ],
      "recent": [
        {
          "ref": "rel:1407",
          "attrs": {
            "subject": "John Williams",
            "American Classical Music Hall of Fame induction year": "2004"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:985",
          "attrs": {
            "subject": "John Williams",
            "occupation": "composer"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:984",
     
```
</details>

### qid=531 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** How many million viewers of the inaugural season of Ultimate Kho Kho (UKK) were from India?
- **gold:** `41 million`
- **arm A answer:** '164 million'
- **arm B answer:** '41 million viewers were from India.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 44dba792.md=True, 139e6e92.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
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
              "ref": "#13365",
              "name": "television debut year",
              "score": 0.71
            },
            {
              "ref": "#2290",
              "name": "inaugural",
              "score": 0.69
            },
            {
              "ref": "#10498",
              "name": "viewers",
              "score": 0.68
            },
            {
              "ref": "#2225",
              "name": "total viewership",
              "score": 0.67
            },
            {
              "ref": "#870",
              "name": "debut year",
              "score": 0.66
            },
            {
              "ref": "#4574",
              "name": "television season",
              "score": 0.66
            },
            {
              "ref": "#13364",
              "name": "television first appearance",
              "score": 0.65
            },
            {
              "ref": "#13394",
              "name": "age in television season 1",
              "score": 0.65
            },
            {
              "ref": "#753",
              "name": "inaugural recipient",
              "score": 0.65
            },
            {
              "ref": "#11604",
              "name": "inaugurated in",
              "score": 0.64
            },
            {
              "ref": "#9115",
              "name": "episode 1 rating",
              "score": 0.64
            },
            {
              "ref": "#7855",
              "name": "inaugural summit start",
              "score": 0.64
            },
            {
              "ref": "#4608",
              "name": "season of",
              "score": 0.63
            },
            {
              "ref": "#14226",
              "name": "presidential audiences first",
              "score": 0.63
            },
            {
              "ref": "#13775",
              "name": "Noorjahan",
              "kind": "work",
              "score": 0.63
            },
            {
              "ref": "#2198",
              "name": "Indian Super League",
              "kind": "sports league",
              "score": 0.63
            },
            {
              "ref": "#13153",
              "name": "first World Championship year",
              "score": 0.63
            },
            {
              "ref": "#9031",
              "name": "Perfect Match season 1",
              "kind": "television season",
              "score": 0.63
            },
            {
              "ref": "#10510",
              "name": "finale viewers",
              "score": 0.63
            },
            {
              "ref": "#3794",
              "name": "began in",
              "score": 0.63
            },
            {
              "ref": "#2500",
              "name": "inaugurated by",
              "score": 0.63
            },
            {
              "ref": "#6292",
              "name": "first voting year",
              "score": 0.63
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#2187",
          "name": "Ultimate Kho Kho",
          "kind": "sports league",
          "summary": "Indian franchise-based kho-kho league abbreviated UKK."
        }
      ],
      "recent": [
        {
          "ref": "rel:4388",
          "attrs": {
            "subject": "Ultimate Kho Kho",
            "hosted by": "Kho Kho Federation of India"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:4394",
          "attrs": {
            "subject": "Ultimate Kho Kho",
            "broadcast partner": "Sony Pictures Network India"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:4384",
          "attrs": {
            "subject": "Ultimate Kho Kho",
            "country": "India"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:4389",
          "attrs": {
            "subject": "Ultimate Kho Kho",
            "most recent champion": "Gujarat Giants"
          },
          "st
```
</details>

### qid=732 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** In which year was Jamini Roy (an Indian painter) awarded the Padma Bhushan by the Government of India?
- **gold:** `1954`
- **arm A answer:** '1955'
- **arm B answer:** 'Jamini Roy was awarded the Padma Bhushan in **1954**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 6690b1f8.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Jamini Roy",
        "Padma Bhushan"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#2419",
          "name": "Jamini Roy",
          "kind": "person",
          "summary": "Indian painter known for a simplified style rooted in Bengali folk traditions."
        },
        {
          "ref": "#2429",
          "name": "Padma Bhushan",
          "kind": "award",
          "summary": "Indian civilian award conferred by the Government of India."
        }
      ],
      "recent": [
        {
          "ref": "rel:5052",
          "attrs": {
            "subject": "Jamini Roy",
            "received award": "Padma Bhushan"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:5053",
          "attrs": {
            "subject": "Jamini Roy",
            "Padma Bhushan year": "1954"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:5012",
          "attrs": {
            "subject": "Jamini Roy",
            "nationality": "Indian"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:5046",
          "attrs": {
            "subject": "Jamini Roy",
            "worked in": "Calcutta"
          },
          "status": "asserted",
          "confidence": 0.98,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:5027",
          "attrs": {
            "subject": "Jamini Roy",
            "influenced by": "Bengali folk traditions"
          },
          "status": "asserted",
          "confidence": 0.99,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  }
]
```
</details>

### qid=796 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** What were the day, month, and year when Google released a version of Chrome on Windows that added hardware-accelerated H.264 video decoding?
- **gold:** `6 November 2012`
- **arm A answer:** '8 February 2012'
- **arm B answer:** 'Google released it on **6 November 2012**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 217cd58e.md=True  · fuzz(gold, frames): 47

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Google Chrome",
        "Windows",
        "hardware-accelerated H.264 video decoding"
      ],
      "limit": 5,
      "history_depth": 2
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[2]",
          "submitted": "hardware-accelerated H.264 video decoding",
          "resolved": false,
          "candidates": [
            {
              "ref": "#2733",
              "name": "hardware-accelerated",
              "score": 0.46
            },
            {
              "ref": "#3166",
              "name": "supports hardware acceleration",
              "score": 0.41
            },
            {
              "ref": "#2730",
              "name": "Windows H.264 decoding added",
              "score": 0.31
            },
            {
              "ref": "#2732",
              "name": "Windows H.264 decoding",
              "score": 0.31
            },
            {
              "ref": "#2684",
              "name": "H.264",
              "kind": "video codec",
              "score": 0.82
            },
            {
              "ref": "#2734",
              "name": "announced H.264 open-sourcing",
              "score": 0.8
            },
            {
              "ref": "#2728",
              "name": "announced H.264 removal",
              "score": 0.73
            },
            {
              "ref": "#2685",
              "name": "video codec",
              "score": 0.73
            },
            {
              "ref": "#2736",
              "name": "H.264 licensing fees",
              "score": 0.71
            },
            {
              "ref": "#2630",
              "name": "x86-64",
              "score": 0.71
            },
            {
              "ref": "#3181",
              "name": "Intel x86",
              "score": 0.69
            },
            {
              "ref": "#2686",
              "name": "Cisco",
              "kind": "organization",
              "score": 0.67
            },
            {
              "ref": "#3350",
              "name": "decoder source code provider",
              "score": 0.67
            },
            {
              "ref": "#869",
              "name": "faster contemporary model",
              "score": 0.66
            },
            {
              "ref": "#3180",
              "name": "first supported mobile architecture",
              "score": 0.66
            },
            {
              "ref": "#1827",
              "name": "produced higher-resolution maps of",
              "score": 0.65
            },
            {
              "ref": "#2238",
              "name": "fast format",
              "score": 0.64
            },
            {
              "ref": "#2556",
              "name": "Blink",
              "kind": "software engine",
              "score": 0.64
            },
            {
              "ref": "#1734",
              "name": "hypothesized collision epoch",
              "score": 0.64
            },
            {
              "ref": "#2683",
              "name": "Mike Jazayeri",
              "kind": "person",
              "score": 0.64
            },
            {
              "ref": "#3158",
              "name": "minimum CPU instruction set",
              "score": 0.64
            },
            {
              "ref": "#3206",
              "name": "GTK",
              "kind": "software",
              "score": 0.64
            },
            {
              "ref": "#3052",
              "name": "CPU",
              "score": 0.64
            },
            {
              "ref": "#2968",
              "name": "invasive-ad blocking introduced",
              "score": 0.63
            },
            {
              "ref": "#2632",
              "name": "ARMv8-A",
              "score": 0.63
            },
            {
              "ref": "#2877",
              "name": "performance feature",
              "score": 0.62
            },
            {
              "ref": "#12069",
              "name": "border-crossing computerization",
              "score": 0.62
            },
            {
              "ref": "#3490",
              "name": "built-in Flash Player",
              "score": 0.62
            },
            {
              "ref": "#1825",
              "name": "early mapping method",
              "score": 0.62
            },
            {
              "ref": "#2251",
              "name": "unrestricted-direction attacker",
              "score": 0.62
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#2551",
          "name": "Google Chrome",
          "kind": "software",
          "summary": "Google's proprietary web browser."
        },
        {
          "ref": "#2568",
          "name": "Microsoft Windows",
          "kind": "operating system",
          "summary": "Microsoft desktop operating system for which Chrome was first released."
        }
      ],
      "recent": [
        {
          "ref": "rel:5558",
          "attrs": {
            "subject": "Google Chrome",
            "Windows H.264 decoding": "hardware-accelerated"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:5557",
          "attrs": {
            "subject": "Google Chrome",
            "Windows H.264 decoding added": "2012-11-06"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:5339",
          "attrs": {
            "subject": "Google Chrome",
            "supports platform": "x86-64"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:6502",
          "attrs": {
            "subject": "Google Chrome",
      
```
</details>

### qid=894 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** In which year did Julian Bradley, the first Black Republican to serve in the Wisconsin Senate and only the second Black Republican to serve in the Wisconsin Legislature, first move to La Crosse, Wisconsin, with his mother?
- **gold:** `1992`
- **arm A answer:** '1994'
- **arm B answer:** 'Julian Bradley moved to La Crosse with his mother in **1992**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: e5b7a803.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Julian Bradley",
        "moved La Crosse Wisconsin mother"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "moved La Crosse Wisconsin mother",
          "resolved": false,
          "candidates": [
            {
              "ref": "#4027",
              "name": "La Crosse County Republican Party",
              "kind": "organization",
              "score": 0.6
            },
            {
              "ref": "#4021",
              "name": "La Crosse",
              "kind": "place",
              "score": 0.57
            },
            {
              "ref": "#4016",
              "name": "University of Wisconsin–La Crosse",
              "kind": "organization",
              "score": 0.5
            },
            {
              "ref": "#4052",
              "name": "year moved to La Crosse",
              "score": 0.43
            },
            {
              "ref": "#4363",
              "name": "Joseph O. Hirschfelder",
              "kind": "person",
              "score": 0.4
            },
            {
              "ref": "#13419",
              "name": "Brotherhood Without Banners",
              "kind": "fictional organization",
              "score": 0.4
            },
            {
              "ref": "#4017",
              "name": "Wisconsin Senate",
              "kind": "organization",
              "score": 0.37
            },
            {
              "ref": "#4030",
              "name": "Scott Walker",
              "kind": "person",
              "score": 0.37
            },
            {
              "ref": "#4051",
              "name": "moved to",
              "score": 0.66
            },
            {
              "ref": "#1963",
              "name": "relocated to",
              "score": 0.65
            },
            {
              "ref": "#9766",
              "name": "through the mother",
              "score": 0.65
            },
            {
              "ref": "#4025",
              "name": "Franklin, Wisconsin",
              "kind": "place",
              "score": 0.64
            },
            {
              "ref": "#10059",
              "name": "subsequently relocated to",
              "score": 0.64
            },
            {
              "ref": "#13052",
              "name": "moved in",
              "score": 0.63
            },
            {
              "ref": "#1123",
              "name": "his mother",
              "score": 0.63
            },
            {
              "ref": "#4032",
              "name": "New Berlin, Wisconsin",
              "kind": "place",
              "score": 0.62
            },
            {
              "ref": "#10016",
              "name": "requested relocation of",
              "score": 0.62
            },
            {
              "ref": "#14188",
              "name": "mother's life",
              "score": 0.62
            },
            {
              "ref": "#10339",
              "name": "immigrated to",
              "score": 0.61
            },
            {
              "ref": "#13380",
              "name": "maternal grandfather",
              "score": 0.61
            },
            {
              "ref": "#9868",
              "name": "residents relocated in 1946",
              "score": 0.61
            },
            {
              "ref": "#10156",
              "name": "requested relocation outside",
              "score": 0.61
            },
            {
              "ref": "#196",
              "name": "mother",
              "score": 0.61
            },
            {
              "ref": "#5211",
              "name": "formerly regarded as motherland",
              "score": 0.61
            },
            {
              "ref": "#9831",
              "name": "later relocation destination",
              "score": 0.6
            },
            {
              "ref": "#13382",
              "name": "maternal aunt",
              "score": 0.6
            },
            {
              "ref": "#10209",
              "name": "original relocated residents alive in 2001",
              "score": 0.6
            },
            {
              "ref": "#349",
              "name": "adoptive parent",
              "score": 0.6
            },
            {
              "ref": "#340",
              "name": "mother died from complications of",
              "score": 0.6
            },
            {
              "ref": "#2165",
              "name": "adopted child",
              "score": 0.59
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#4013",
          "name": "Julian Bradley",
          "kind": "person",
          "summary": "American Republican politician, businessman, and former professional wrestler born in 1981."
        }
      ],
      "recent": [
        {
          "ref": "rel:8550",
          "attrs": {
            "subject": "Julian Bradley",
            "moved to": "La Crosse"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:8567",
          "attrs": {
            "subject": "Julian Bradley",
            "relocated to": "Franklin, Wisconsin"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:8551",
          "attrs": {
            "subject": "Julian Bradley",
            "year moved to La Crosse": "1992"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:8552",
          "attrs": {
            "subject": "Julian Bradley",
            "graduated from": "La Crosse Central High School"
         
```
</details>

### qid=954 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** When (month-day-year) was "Amazing Love" by the Mississippi Mass Choir released?
- **gold:** `June 4, 2002`
- **arm A answer:** '01-01-1995'
- **arm B answer:** '06-04-2002'

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
          "ref": "rel:9200",
          "attrs": {
            "subject": "Mississippi Mass Choir",
            "based in": "Jackson, Mississippi"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9234",
          "attrs": {
            "subject": "...Then Sings My Soul",
            "artist": "Mississippi Mass Choir"
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
          "ref": "rel:9202",
          "attrs": {
            "subject": "Mississippi Mass Choir",
            "active since": "1988"
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
          "ref": "rel:9684",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "date of death": "2016-10-05"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9685",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "age at death": "72"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9683",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "date of birth": "1944-08-31"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9686",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "nationality": "English"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:9687",
          "attrs": {
            "subject": "Clifford James Cunnell",
            "occupation": "cricketer"
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

### qid=1287 — FIXED  (A=not_attempted → B=correct)  — ✓ grounded

- **Q:** What was the overall length, in inches, of the 1934 Bendix SWC concept car?
- **gold:** `204`
- **arm A answer:** "I don't know."
- **arm B answer:** 'The 1934 Bendix SWC concept car was **204 inches** long overall.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: a8c7f2a3.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "1934 Bendix SWC",
        "overall length inches"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[0]",
          "submitted": "1934 Bendix SWC",
          "resolved": false,
          "candidates": [
            {
              "ref": "#4468",
              "name": "Bendix SWC",
              "kind": "vehicle",
              "score": 0.77
            },
            {
              "ref": "#4469",
              "name": "Bendix Corporation",
              "kind": "organization",
              "score": 0.69
            },
            {
              "ref": "#4470",
              "name": "Alfred M. Ney",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#4471",
              "name": "Vincent Bendix",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#4474",
              "name": "Steel Wheel Corporation",
              "kind": "organization",
              "score": 0.69
            },
            {
              "ref": "#4475",
              "name": "Ottavio Capra",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#4476",
              "name": "Nathan Byer",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#4477",
              "name": "Charles Lair",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#2046",
              "name": "1934",
              "score": 0.76
            },
            {
              "ref": "#4479",
              "name": "William F. Ortwig",
              "kind": "person",
              "score": 0.71
            },
            {
              "ref": "#4485",
              "name": "Continental L-head Series 25A",
              "kind": "engine",
              "score": 0.7
            },
            {
              "ref": "#4550",
              "name": "November 1934",
              "score": 0.7
            },
            {
              "ref": "#4472",
              "name": "Victor Kliesrath",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#4486",
              "name": "Bendix Finger-Tip Control",
              "kind": "technology",
              "score": 0.69
            },
            {
              "ref": "#4483",
              "name": "Studebaker National Museum",
              "kind": "organization",
              "score": 0.68
            },
            {
              "ref": "#4556",
              "name": "Bendix ownership stake",
              "score": 0.68
            },
            {
              "ref": "#4480",
              "name": "Bendix Automotive Development Center",
              "kind": "place",
              "score": 0.68
            },
            {
              "ref": "#4473",
              "name": "Peerless Motor Company",
              "kind": "organization",
              "score": 0.68
            },
            {
              "ref": "#4484",
              "name": "Honeywell Corporation",
              "kind": "organization",
              "score": 0.67
            },
            {
              "ref": "#4482",
              "name": "Gene Wadzinski",
              "kind": "person",
              "score": 0.67
            },
            {
              "ref": "#4478",
              "name": "Fred Thomer",
              "kind": "person",
              "score": 0.67
            },
            {
              "ref": "#4487",
              "name": "SS Bremen",
              "kind": "ship",
              "score": 0.66
            },
            {
              "ref": "#1363",
              "name": "early 1930s",
              "score": 0.66
            },
            {
              "ref": "#12747",
              "name": "1934-06-11",
              "score": 0.65
            },
            {
              "ref": "#2036",
              "name": "1934-02-28",
              "score": 0.63
            },
            {
              "ref": "#4562",
              "name": "South Bend Bendix Proving Grounds",
              "score": 0.63
            },
            {
              "ref": "#2044",
              "name": "Georgi_Dimitrov: Head of Comintern, 1934",
              "kind": "pointer",
              "score": 0.61
            },
            {
              "ref": "#1968",
              "name": "1932",
              "score": 0.61
            },
            {
              "ref": "#2458",
              "name": "early 1920s",
              "score": 0.61
            },
            {
              "ref": "#5597",
              "name": "1905",
              "score": 0.6
            }
          ]
        },
        {
          "at": "focus[1]",
          "submitted": "overall length inches",
          "resolved": false,
          "candidates": [
            {
              "ref": "#9269",
              "name": "average length",
              "score": 0.37
            },
            {
              "ref": "#91",
              "name": "overall edition",
              "score": 0.32
            },
            {
              "ref": "#443",
              "name": "Classical Walk of Fame",
              "kind": "place",
              "score": 0.32
            },
            {
              "ref": "#4919",
              "name": "Taiwan Province",
              "kind": "place",
              "score": 0.32
            },
            {
              "ref": "#5180",
              "name": "The Confession of Lee Teng-hui’s Administration",
              "kind": "work",
              "score": 0.32
            },
            {
              "ref": "#5547",
              "name": "Lorentz–FitzGerald contraction",
              "kind": "concept",
              "score": 0.32
            },
    
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
              "ref": "#3585",
              "name": "original address",
              "score": 0.66
            },
            {
              "ref": "#5481",
              "name": "Camden House Grammar School",
              "score": 0.66
            },
            {
              "ref": "#3576",
              "name": "Archway Road, London",
              "score": 0.64
            },
            {
              "ref": "#5467",
              "name": "Camden Town",
              "score": 0.63
            },
            {
              "ref": "#12427",
              "name": "St John-at-Hampstead",
              "kind": "place",
              "score": 0.62
            },
            {
              "ref": "#477",
              "name": "address",
              "score": 0.62
            },
            {
              "ref": "#13551",
              "name": "ancestral home",
              "score": 0.62
            },
            {
              "ref": "#3586",
              "name": "193 Grove Road, London",
              "score": 0.61
            },
            {
              "ref": "#5468",
              "name": "birth address",
              "score": 0.61
            },
            {
              "ref": "#5915",
              "name": "upper house",
              "score": 0.61
            },
            {
              "ref": "#5390",
              "name": "residential neighbourhood",
              "score": 0.6
            },
            {
              "ref": "#3550",
              "name": "Ilford, Essex, England",
              "score": 0.6
            },
            {
              "ref": "#11260",
              "name": "address date",
              "score": 0.6
            },
            {
              "ref": "#13327",
              "name": "House Stark",
              "kind": "fictional house",
              "score": 0.59
            },
            {
              "ref": "#12620",
              "name": "introduced to London contacts",
              "score": 0.59
            },
            {
              "ref": "#4739",
              "name": "resting place",
              "score": 0.59
            },
            {
              "ref": "#2747",
              "name": "address and search",
              "score": 0.59
            },
            {
              "ref": "#9870",
              "name": "living elsewhere in 1946",
              "score": 0.59
            },
            {
              "ref": "#12026",
              "name": "home delivery",
              "score": 0.59
            },
            {
              "ref": "#10210",
              "name": "principal residence",
              "score": 0.59
            },
            {
              "ref": "#5916",
              "name": "lower house",
              "score": 0.59
            },
            {
              "ref": "#12556",
              "name": "London base",
              "score": 0.58
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#4641",
          "name": "Margaret Gardiner",
          "kind": "person",
          "summary": "British art patron, collector, political activist, writer, and founder of the Pier Arts Centre (1904–2005)."
        }
      ],
      "recent": [
        {
          "ref": "rel:10161",
          "attrs": {
            "subject": "Margaret Gardiner",
            "residence": "Hampstead"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:10162",
          "attrs": {
            "subject": "Margaret Gardiner",
            "Hampstead residence start year": "1932"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:10159",
          "attrs": {
            "subject": "Margaret Gardiner",
            "home": "35 Downshire Hill"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:10131",
          "attrs": {
            "subject": "Margaret Gardiner",
            "citizenship": "British"
          },
          "status": "asserted",
    
```
</details>

### qid=1605 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** Which Chinese president was nicknamed "Mr. Democracy?"
- **gold:** `Lee Teng-hui.`
- **arm A answer:** 'Li Yuanhong.'
- **arm B answer:** 'Lee Teng-hui, former president of the Republic of China (Taiwan), was nicknamed “Mr. Democracy.”'

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
              "ref": "#5163",
              "name": "Presidential Office of the Republic of China",
              "kind": "organization",
              "score": 0.78
            },
            {
              "ref": "#4916",
              "name": "Chiang Kai-shek",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#4917",
              "name": "Yen Chia-kan",
              "kind": "person",
              "score": 0.76
            },
            {
              "ref": "#2109",
              "name": "Wang Ming",
              "kind": "person",
              "score": 0.75
            },
            {
              "ref": "#4915",
              "name": "Executive Yuan",
              "kind": "organization",
              "score": 0.73
            },
            {
              "ref": "#4712",
              "name": "Chiang Ching-kuo",
              "kind": "person",
              "score": 0.72
            },
            {
              "ref": "#8848",
              "name": "Jacques Chirac",
              "kind": "person",
              "score": 0.72
            },
            {
              "ref": "#4714",
              "name": "Lee Yuan-tsu",
              "kind": "person",
              "score": 0.72
            },
            {
              "ref": "#5026",
              "name": "succeeded as Legislative Yuan president",
              "score": 0.71
            },
            {
              "ref": "#5162",
              "name": "Chen I-hsin",
              "kind": "person",
              "score": 0.7
            },
            {
              "ref": "#4977",
              "name": "Soong Mei-ling",
              "kind": "person",
              "score": 0.7
            },
            {
              "ref": "#6682",
              "name": "president",
              "score": 0.7
            },
            {
              "ref": "#5075",
              "name": "Wang Ching-feng",
              "kind": "person",
              "score": 0.7
            },
            {
              "ref": "#4743",
              "name": "president of",
              "score": 0.7
            },
            {
              "ref": "#4710",
              "name": "Republic of China",
              "kind": "country",
              "score": 0.7
            },
            {
              "ref": "#5167",
              "name": "Tsai Ing-wen",
              "kind": "person",
              "score": 0.7
            },
            {
              "ref": "#5948",
              "name": "presidential",
              "score": 0.69
            },
            {
              "ref": "#6499",
              "name": "U.S. president",
              "score": 0.69
            },
            {
              "ref": "#4719",
              "name": "Chiu Chuang-huan",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#4985",
              "name": "Sun Li-jen",
              "kind": "person",
              "score": 0.68
            },
            {
              "ref": "#5877",
              "name": "President of Pakistan",
              "kind": "office",
              "score": 0.68
            },
            {
              "ref": "#5082",
              "name": "Republic of Taiwan",
              "kind": "concept",
              "score": 0.67
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#5304",
          "name": "Mr. Democracy"
        }
      ],
      "recent": [
        {
          "ref": "rel:11590",
          "attrs": {
            "subject": "Lee Teng-hui",
            "nickname": "Mr. Democracy"
          },
          "status": "asserted",
          "confidence": 0.99,
          "support_count": 1,
          "date": "2024-07-03"
        }
      ]
    }
  }
]
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
        "described by Brisout"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "described by Brisout",
          "resolved": false,
          "candidates": [
            {
              "ref": "#8620",
              "name": "Plan for post-Saddam Iraq",
              "kind": "work",
              "score": 0.67
            },
            {
              "ref": "#10386",
              "name": "Carl Gustaf Mannerheim",
              "kind": "person",
              "score": 0.61
            },
            {
              "ref": "#10387",
              "name": "Pierre François Marie Auguste Dejean",
              "kind": "person",
              "score": 0.61
            },
            {
              "ref": "#10388",
              "name": "Gotthelf Fischer von Waldheim",
              "kind": "person",
              "score": 0.61
            },
            {
              "ref": "#9251",
              "name": "described by",
              "score": 0.56
            },
            {
              "ref": "#10431",
              "name": "described specimens collected by",
              "score": 0.56
            },
            {
              "ref": "#12730",
              "name": "first formally described by",
              "score": 0.56
            },
            {
              "ref": "#885",
              "name": "Shraddhadeva Manu",
              "kind": "deity",
              "score": 0.5
            },
            {
              "ref": "#5986",
              "name": "Brisout",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#9250",
              "name": "described in",
              "score": 0.65
            },
            {
              "ref": "#13614",
              "name": "described as",
              "score": 0.64
            },
            {
              "ref": "#13117",
              "name": "first explicitly described as",
              "score": 0.63
            },
            {
              "ref": "#12731",
              "name": "first formally described in",
              "score": 0.63
            },
            {
              "ref": "#8097",
              "name": "described",
              "score": 0.62
            },
            {
              "ref": "#12278",
              "name": "Leonid Brezhnev",
              "kind": "person",
              "score": 0.61
            },
            {
              "ref": "#12511",
              "name": "Eugène Delacroix",
              "kind": "person",
              "score": 0.61
            },
            {
              "ref": "#11598",
              "name": "coined description",
              "score": 0.6
            },
            {
              "ref": "#1024",
              "name": "describes horses of",
              "score": 0.6
            },
            {
              "ref": "#1367",
              "name": "named by",
              "score": 0.6
            },
            {
              "ref": "#13427",
              "name": "Polliver",
              "kind": "fictional character",
              "score": 0.59
            },
            {
              "ref": "#1511",
              "name": "introduced term",
              "score": 0.59
            },
            {
              "ref": "#14284",
              "name": "described as spiritual",
              "score": 0.59
            },
            {
              "ref": "#4950",
              "name": "Lee_Teng-hui, chunk 3, fourth paragraph",
              "kind": "pointer",
              "score": 0.58
            },
            {
              "ref": "#5555",
              "name": "Kennelly–Heaviside layer",
              "kind": "concept",
              "score": 0.58
            },
            {
              "ref": "#10657",
              "name": "coined term",
              "score": 0.58
            },
            {
              "ref": "#1071",
              "name": "revealed identity of",
              "score": 0.58
            },
            {
              "ref": "#5543",
              "name": "Norbert Wiener",
              "kind": "person",
              "score": 0.58
            },
            {
              "ref": "#8778",
              "name": "William R. Clark",
              "kind": "person",
              "score": 0.58
            },
            {
              "ref": "#13072",
              "name": "January–February 1889",
              "score": 0.58
            },
            {
              "ref": "#6206",
              "name": "named for",
              "score": 0.58
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#5977",
          "name": "Eremiaphila barbara",
          "kind": "taxon",
          "summary": "A species of praying mantis in the family Eremiaphilidae."
        }
      ],
      "recent": [
        {
          "ref": "rel:13307",
          "attrs": {
            "subject": "Eremiaphila barbara",
            "taxon rank": "species"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:13315",
          "attrs": {
            "subject": "Eremiaphila barbara",
            "binomial authority": "Brisout"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:13314",
          "attrs": {
            "subject": "Eremiaphila barbara",
            "genus": "Eremiaphila"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:13316",
          "attrs": {
            "subject": "Eremiaphila barbara",
            "descripti
```
</details>

### qid=1875 — FIXED  (A=incorrect → B=correct)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** Who did Theodore von Kármán select as his research assistant when he accepted the directorship of the Guggenheim Aeronautical Laboratory at the California Institute of Technology in 1930?
- **gold:** `Frank Wattendorf`
- **arm A answer:** 'Clark B. Millikan.'
- **arm B answer:** 'He selected **Frank Wattendorf** as his research assistant.'

- grounding (date-aware) — in store: **True** · in frames B saw: **False** · on pages: 4403b822.md=True, 5a953097.md=True  · fuzz(gold, frames): 44

  - [x] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Theodore von Kármán",
        "research assistant",
        "Guggenheim Aeronautical Laboratory",
        "1930"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#5998",
          "name": "Theodore von Kármán",
          "kind": "person",
          "summary": "Hungarian-American mathematician, aerospace engineer, and physicist noted for aerodynamics and astronautics."
        },
        {
          "ref": "#6242",
          "name": "research assistant"
        },
        {
          "ref": "#6228",
          "name": "Guggenheim Aeronautical Laboratory",
          "kind": "organization",
          "summary": "Caltech aeronautical laboratory directed by Theodore von Kármán beginning in 1930."
        },
        {
          "ref": "#1364",
          "name": "1930"
        }
      ],
      "recent": [
        {
          "ref": "rel:14021",
          "attrs": {
            "subject": "Theodore von Kármán",
            "director of": "Guggenheim Aeronautical Laboratory"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:13354",
          "attrs": {
            "subject": "Theodore von Kármán",
            "occupation": "aerospace engineer"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 2,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:13357",
          "attrs": {
            "subject": "Theodore von Kármán",
            "worked in": "aeronautics"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:13356",
          "attrs": {
            "subject": "Theodore von Kármán",
            "field": "aerospace engineering"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:13358",
          "attrs": {
            "subject": "Theodore von Kármán",
            "worked in": "astronautics"
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

### qid=2082 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** What is the name of the recipient of the Lapworth Medal in 2004?
- **gold:** `James Valentine`
- **arm A answer:** 'Professor John C. W. Cope'
- **arm B answer:** 'The 2004 recipient of the Lapworth Medal was **James Valentine**.'

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
          "ref": "rel:14713",
          "attrs": {
            "subject": "Lapworth Medal",
            "2016 recipient": "Adrian William Amsler Rushton"
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
        },
        {
          "ref": "rel:14709",
          "attrs": {
            "subject": "Lapworth Medal",
            "2020 recipient": "Andrew B. Smith"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:14705",
          "attrs": {
            "subject": "Lapworth Medal",
            "2024 recipient": "Michael J. Benton"
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

### qid=2673 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** What is the father's name of the 2015 MLA of Matia Mahal?
- **gold:** `Shamim Ahmed Khan`
- **arm A answer:** 'Abdul Wajid Khan'
- **arm B answer:** 'The 2015 MLA for Matia Mahal was **Asim Ahmed Khan**. His father’s name is **Shamim Ahmed Khan**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 8f7e32c2.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Matia Mahal",
        "2015 MLA",
        "father's name"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "2015 MLA",
          "resolved": false,
          "candidates": [
            {
              "ref": "#10600",
              "name": "2015 Muslim vote share",
              "score": 0.67
            },
            {
              "ref": "#242",
              "name": "2015-11-05/2019-09-11",
              "score": 0.5
            },
            {
              "ref": "#246",
              "name": "2015-11-05",
              "score": 0.5
            },
            {
              "ref": "#249",
              "name": "2010-09-30/2015-08-02",
              "score": 0.5
            },
            {
              "ref": "#259",
              "name": "2015 election opponent defeated",
              "score": 0.5
            },
            {
              "ref": "#265",
              "name": "2015-04-23",
              "score": 0.5
            },
            {
              "ref": "#368",
              "name": "2015 Canadian federal election in Flamborough—Glanbrook",
              "kind": "event",
              "score": 0.5
            },
            {
              "ref": "#645",
              "name": "2015 Grammy win — Best Instrumental Composition",
              "score": 0.5
            },
            {
              "ref": "#225",
              "name": "2015",
              "score": 0.76
            },
            {
              "ref": "#3504",
              "name": "late 2015",
              "score": 0.72
            },
            {
              "ref": "#7831",
              "name": "2015-03",
              "score": 0.71
            },
            {
              "ref": "#6536",
              "name": "2015 recipient",
              "score": 0.71
            },
            {
              "ref": "#10598",
              "name": "AAP Muslim MLAs",
              "score": 0.71
            },
            {
              "ref": "#10574",
              "name": "Delhi MLA",
              "score": 0.7
            },
            {
              "ref": "#5205",
              "name": "2015-07",
              "score": 0.7
            },
            {
              "ref": "#7814",
              "name": "2015-06",
              "score": 0.69
            },
            {
              "ref": "#10588",
              "name": "2015 election party",
              "score": 0.69
            },
            {
              "ref": "#10589",
              "name": "2015 election constituency",
              "score": 0.68
            },
            {
              "ref": "#10590",
              "name": "2015 election votes",
              "score": 0.68
            },
            {
              "ref": "#113",
              "name": "15",
              "score": 0.68
            },
            {
              "ref": "#5353",
              "name": "August 2015",
              "score": 0.68
            },
            {
              "ref": "#6324",
              "name": "2015 Word of the Year",
              "score": 0.68
            },
            {
              "ref": "#10577",
              "name": "February 2015",
              "score": 0.68
            },
            {
              "ref": "#10592",
              "name": "2015 victory margin",
              "score": 0.68
            },
            {
              "ref": "#6937",
              "name": "2015 Senate seats",
              "score": 0.67
            },
            {
              "ref": "#3076",
              "name": "2015-04-29",
              "score": 0.67
            },
            {
              "ref": "#3135",
              "name": "2015-04-14",
              "score": 0.67
            },
            {
              "ref": "#1288",
              "name": "2015-07-14",
              "score": 0.66
            },
            {
              "ref": "#8182",
              "name": "cited policy",
              "score": 0.66
            },
            {
              "ref": "#6979",
              "name": "2015 Gilgit-Baltistan Assembly seats",
              "score": 0.66
            }
          ]
        },
        {
          "at": "focus[2]",
          "submitted": "father's name",
          "resolved": false,
          "candidates": [
            {
              "ref": "#1311",
              "name": "Falconer Madan",
              "kind": "person",
              "score": 0.82
            },
            {
              "ref": "#890",
              "name": "Sanjna",
              "kind": "deity",
              "score": 0.73
            },
            {
              "ref": "#1001",
              "name": "Rajni",
              "kind": "deity",
              "score": 0.64
            },
            {
              "ref": "#1056",
              "name": "names father of Yama",
              "score": 0.64
            },
            {
              "ref": "#147",
              "name": "Gordon Sweet",
              "kind": "person",
              "score": 0.55
            },
            {
              "ref": "#498",
              "name": "Walt Disney",
              "kind": "person",
              "score": 0.55
            },
            {
              "ref": "#1106",
              "name": "Vishvakarma",
              "kind": "deity",
              "score": 0.55
            },
            {
              "ref": "#3934",
              "name": "Albert Stauch",
              "kind": "person",
              "score": 0.55
            },
            {
              "ref": "#1032",
              "name": "father of",
              "score": 0.83
            },
            {
              "ref": "#194",
              "name": "father",
              "score": 0.81
            },
            {
              "ref": "#13376",
              "name": "paternal grandfather",
              "score": 0.74
            },
    
```
</details>

### qid=2835 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** From what year in the 1800s did Nikolai Christian Grove Prebensen serve as the County Governor of Aust-Agder in Norway?
- **gold:** `1896`
- **arm A answer:** '1863'
- **arm B answer:** 'Nikolai Christian Grove Prebensen began serving as County Governor of Aust-Agder in **1896**.'

- grounding (date-aware) — in store: **True** · in frames B saw: **True** · on pages: 9ba78da4.md=True  · fuzz(gold, frames): 100

  - [x] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Nikolai Christian Grove Prebensen",
        "County Governor Aust-Agder"
      ],
      "limit": 5,
      "history_depth": 2
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "County Governor Aust-Agder",
          "resolved": false,
          "candidates": [
            {
              "ref": "#10801",
              "name": "Aust-Agder governor term began",
              "score": 0.67
            },
            {
              "ref": "#10802",
              "name": "Aust-Agder governor term ended",
              "score": 0.67
            },
            {
              "ref": "#10793",
              "name": "county governor of",
              "score": 0.58
            },
            {
              "ref": "#10759",
              "name": "Finnmark",
              "kind": "place",
              "score": 0.5
            },
            {
              "ref": "#10764",
              "name": "Aust-Agder",
              "kind": "place",
              "score": 0.5
            },
            {
              "ref": "#14051",
              "name": "Price Daniel",
              "kind": "person",
              "score": 0.42
            },
            {
              "ref": "#6726",
              "name": "Pakistan People's Party nationalization program",
              "kind": "policy",
              "score": 0.38
            },
            {
              "ref": "#7341",
              "name": "Illinois payday loan regulations",
              "kind": "policy",
              "score": 0.38
            },
            {
              "ref": "#4967",
              "name": "governor of",
              "score": 0.69
            },
            {
              "ref": "#10013",
              "name": "military governor of",
              "score": 0.65
            },
            {
              "ref": "#4968",
              "name": "became governor year",
              "score": 0.65
            },
            {
              "ref": "#5745",
              "name": "State Bank governor ordinal",
              "score": 0.64
            },
            {
              "ref": "#5752",
              "name": "appointed State Bank governor",
              "score": 0.63
            },
            {
              "ref": "#11747",
              "name": "Governor of Neuquén Province",
              "score": 0.62
            },
            {
              "ref": "#5750",
              "name": "State Bank governor predecessor",
              "score": 0.62
            },
            {
              "ref": "#6864",
              "name": "imposed governor's rule in",
              "score": 0.62
            },
            {
              "ref": "#5235",
              "name": "Governor of Taiwan Province",
              "score": 0.62
            },
            {
              "ref": "#5746",
              "name": "State Bank governor start",
              "score": 0.61
            },
            {
              "ref": "#10794",
              "name": "Finnmark governor term began",
              "score": 0.61
            },
            {
              "ref": "#5748",
              "name": "State Bank governor end",
              "score": 0.61
            },
            {
              "ref": "#5751",
              "name": "State Bank governor successor",
              "score": 0.6
            },
            {
              "ref": "#10795",
              "name": "Finnmark governor term ended",
              "score": 0.59
            },
            {
              "ref": "#10536",
              "name": "legislature",
              "score": 0.58
            },
            {
              "ref": "#159",
              "name": "Russ Powers",
              "kind": "person",
              "score": 0.58
            },
            {
              "ref": "#8720",
              "name": "Richard Armitage",
              "kind": "person",
              "score": 0.58
            },
            {
              "ref": "#10050",
              "name": "administering country",
              "score": 0.57
            },
            {
              "ref": "#4775",
              "name": "mayor of",
              "score": 0.57
            },
            {
              "ref": "#12603",
              "name": "appointed Visitor at",
              "score": 0.57
            },
            {
              "ref": "#10810",
              "name": "minister",
              "score": 0.57
            },
            {
              "ref": "#7411",
              "name": "reelected to",
              "score": 0.57
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#10750",
          "name": "Nikolai Prebensen",
          "kind": "person",
          "summary": "Norwegian Conservative politician and civil servant (1850–1938)."
        }
      ],
      "recent": [
        {
          "ref": "rel:24626",
          "attrs": {
            "subject": "Nikolai Prebensen",
            "county governor of": "Aust-Agder"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24627",
          "attrs": {
            "subject": "Nikolai Prebensen",
            "Aust-Agder governor term began": "1896"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24628",
          "attrs": {
            "subject": "Nikolai Prebensen",
            "Aust-Agder governor term ended": "1906"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24615",
          "attrs": {
            "subject": "Nikolai Prebensen",
            "county governor of": "Finnmark"
          },
          "status": "asserted",

```
</details>

### qid=2857 — FIXED  (A=incorrect → B=correct)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** In what year was Peter Scholze appointed the Chancellor's Professor at the University of California, Berkeley?
- **gold:** `2014`
- **arm A answer:** '2019'
- **arm B answer:** 'Peter Scholze was appointed a Chancellor’s Professor at the University of California, Berkeley, in **2014**.'

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
        "Chancellor's Professor"
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
        }
      ],
      "recent": [
        {
          "ref": "rel:24789",
          "attrs": {
            "subject": "Peter Scholze",
            "chancellor's professor at": "University of California, Berkeley"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24782",
          "attrs": {
            "subject": "Peter Scholze",
            "professor at": "University of Bonn"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24774",
          "attrs": {
            "subject": "Peter Scholze",
            "occupation": "mathematician"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24777",
          "attrs": {
            "subject": "Peter Scholze",
            "alma mater": "University of Bonn"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24773",
          "attrs": {
            "subject": "Peter Scholze",
            "nationality": "German"
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
              "ref": "#11006",
              "name": "William H. Twenhofel Medal year",
              "score": 0.88
            },
            {
              "ref": "#10903",
              "name": "William R. Dickinson",
              "kind": "person",
              "score": 0.83
            },
            {
              "ref": "#10909",
              "name": "William Hay",
              "kind": "person",
              "score": 0.82
            },
            {
              "ref": "#10904",
              "name": "William L. Fisher",
              "kind": "person",
              "score": 0.81
            },
            {
              "ref": "#10891",
              "name": "Hans E. Reineck",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10920",
              "name": "Judith Ann McKenzie",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10918",
              "name": "Robert W. Dalrymple",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10893",
              "name": "James Lee Wilson",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10885",
              "name": "Alfred G. Fischer",
              "kind": "person",
              "score": 0.78
            },
            {
              "ref": "#10883",
              "name": "Laurence L. Sloss",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10899",
              "name": "Grover E. Murray",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10896",
              "name": "Robert H. Dott, Jr.",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10897",
              "name": "Harold G. Reading",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10889",
              "name": "Franklyn B. Van Houten",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10906",
              "name": "Gerard V. Middleton",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10908",
              "name": "Wolfgang Schlager",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10892",
              "name": "Kenneth O. Emery",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10925",
              "name": "David Bottjer",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10898",
              "name": "Robert J. Weimer",
              "kind": "person",
              "score": 0.77
            },
            {
              "ref": "#10894",
              "name": "John Imbrie",
              "kind": "person",
              "score": 0.76
            },
            {
              "ref": "#10902",
              "name": "Lloyd C. Pray",
              "kind": "person",
              "score": 0.76
            },
            {
              "ref": "#10917",
              "name": "John Southard",
              "kind": "person",
              "score": 0.76
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#10900",
          "name": "Gerald M. Friedman",
          "kind": "person",
          "summary": "German-born American geologist and professor associated with modern sedimentology."
        }
      ],
      "recent": [
        {
          "ref": "rel:25241",
          "attrs": {
            "subject": "Gerald M. Friedman",
            "received award": "William H. Twenhofel Medal"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:25007",
          "attrs": {
            "subject": "William H. Twenhofel Medal",
            "1997 recipient": "Gerald M. Friedman"
          }
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
              "ref": "#2460",
              "name": "1921",
              "score": 0.71
            },
            {
              "ref": "#9979",
              "name": "1920",
              "score": 0.61
            },
            {
              "ref": "#2458",
              "name": "early 1920s",
              "score": 0.6
            },
            {
              "ref": "#2479",
              "name": "London exhibition year",
              "score": 0.6
            },
            {
              "ref": "#3557",
              "name": "Turner Prize year",
              "score": 0.6
            },
            {
              "ref": "#3536",
              "name": "Charles Saatchi",
              "kind": "person",
              "score": 0.6
            },
            {
              "ref": "#10225",
              "name": "cargo ships purchased",
              "score": 0.6
            },
            {
              "ref": "#2477",
              "name": "British India Street exhibition year",
              "score": 0.59
            },
            {
              "ref": "#13042",
              "name": "mid-1882",
              "score": 0.59
            },
            {
              "ref": "#12584",
              "name": "paintings sold in England during lifetime",
              "score": 0.59
            },
            {
              "ref": "#1920",
              "name": "1919",
              "score": 0.59
            },
            {
              "ref": "#1363",
              "name": "early 1930s",
              "score": 0.59
            },
            {
              "ref": "#4539",
              "name": "1931",
              "score": 0.58
            },
            {
              "ref": "#5685",
              "name": "1899",
              "score": 0.58
            },
            {
              "ref": "#3622",
              "name": "Tate Britain",
              "kind": "organization",
              "score": 0.58
            },
            {
              "ref": "#12538",
              "name": "purchase year",
              "score": 0.58
            },
            {
              "ref": "#10687",
              "name": "museum",
              "score": 0.58
            },
            {
              "ref": "#1968",
              "name": "1932",
              "score": 0.58
            },
            {
              "ref": "#10426",
              "name": "1830",
              "score": 0.58
            },
            {
              "ref": "#4673",
              "name": "Margaret_Gardiner_(art_collector)",
              "kind": "pointer",
              "score": 0.58
            },
            {
              "ref": "#12466",
              "name": "31 March 1837",
              "score": 0.58
            },
            {
              "ref": "#1429",
              "name": "1840s",
              "score": 0.58
            },
            {
              "ref": "#6583",
              "name": "1876",
              "score": 0.58
            },
            {
              "ref": "#13534",
              "name": "used coin for passage to",
              "score": 0.58
            },
            {
              "ref": "#12515",
              "name": "Henry Vaughan",
              "kind": "person",
              "score": 0.57
            },
            {
              "ref": "#5534",
              "name": "1922",
              "score": 0.57
            },
            {
              "ref": "#9717",
              "name": "World Heritage reference",
              "score": 0.57
            },
            {
              "ref": "#13111",
              "name": "early 1896",
              "score": 0.57
            }
          ]
        },
        {
          "at": "focus[2]",
          "submitted": "English guineas",
          "ref": "#12148",
          "name": "1480 English guineas",
          "via": "lexical",
          "score": 1
        }
      ],
      "focus": [
        {
          "ref": "#12125",
          "name": "Bust of Thomas Baker",
          "kind": "sculpture",
          "summary": "A 1638 marble portrait bust of Thomas Baker attributed to Gian Lorenzo Bernini and his workshop."
        },
        {
          "ref": "#12148",
          "name": "1480 English guineas"
        }
      ],
      "recent": [
        {
          "ref": "rel:27960",
          "attrs": {
            "subject": "Bust of Thomas Baker",
            "purchase price": "1480 English guineas"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:27958",
          "attrs": {
            "subject": "Victoria and Albert Museum",
            "acquired": "Bust of Thomas Baker"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:27956",
          "attrs": {
            "subject": "Bust of Thomas Baker",
            "held by": "Victoria and Albert Museum"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel
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
              "ref": "#11068",
              "name": "renamed to current name",
              "score": 0.82
            },
            {
              "ref": "#2176",
              "name": "renamed to",
              "score": 0.81
            },
            {
              "ref": "#11443",
              "name": "renamed",
              "score": 0.79
            },
            {
              "ref": "#2177",
              "name": "renamed in",
              "score": 0.78
            },
            {
              "ref": "#12734",
              "name": "renamed by",
              "score": 0.77
            },
            {
              "ref": "#12733",
              "name": "original name",
              "score": 0.77
            },
            {
              "ref": "#9709",
              "name": "former name",
              "score": 0.76
            },
            {
              "ref": "#13064",
              "name": "adopted first name",
              "score": 0.75
            },
            {
              "ref": "#2178",
              "name": "name restored as",
              "score": 0.75
            },
            {
              "ref": "#10433",
              "name": "renamed year",
              "score": 0.72
            },
            {
              "ref": "#2659",
              "name": "name origin",
              "score": 0.71
            },
            {
              "ref": "#10736",
              "name": "coined name",
              "score": 0.71
            },
            {
              "ref": "#5081",
              "name": "Name Rectification Campaign",
              "kind": "movement",
              "score": 0.7
            },
            {
              "ref": "#12736",
              "name": "later name",
              "score": 0.7
            },
            {
              "ref": "#9710",
              "name": "former-name end",
              "score": 0.7
            },
            {
              "ref": "#1346",
              "name": "first name suggestion by",
              "score": 0.7
            },
            {
              "ref": "#1305",
              "name": "promoted name",
              "score": 0.7
            },
            {
              "ref": "#13686",
              "name": "name bestowed by",
              "score": 0.7
            },
            {
              "ref": "#2172",
              "name": "named in",
              "score": 0.69
            },
            {
              "ref": "#3374",
              "name": "changes",
              "score": 0.69
            },
            {
              "ref": "#10434",
              "name": "former namesake",
              "score": 0.69
            },
            {
              "ref": "#1368",
              "name": "naming year",
              "score": 0.68
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#12679",
          "name": "George Bentham",
          "kind": "person",
          "summary": "The botanist stated to have changed the species name in 1865."
        },
        {
          "ref": "#12677",
          "name": "Hedaroma pinifolium",
          "kind": "taxon",
          "summary": "The original name given to Darwinia pinifolia in 1839."
        },
        {
          "ref": "#12680",
          "name": "Pimelea pinifolia",
          "kind": "taxon",
          "summary": "The name stated to have been applied to the species by George Bentham in 1865."
        }
      ],
      "recent": [
        {
          "ref": "rel:29670",
          "attrs": {
            "subject": "Darwinia pinifolia",
            "renamed by": "George Bentham"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:29672",
          "attrs": {
            "subject": "Darwinia pinifolia",
            "later name": "Pimelea pinifolia"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:29669",
          "attrs": {
            "subject": "Darwinia pinifolia",
            "original name": "Hedaroma pinifolium"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        }
   
```
</details>

### qid=3710 — FIXED  (A=incorrect → B=correct)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** Who was the inaugural holder of the position of Attorney General of Guatemala?
- **gold:** `Ramses Cuestas Gomez`
- **arm A answer:** 'Acisclo Valladares Molina'
- **arm B answer:** 'The inaugural Attorney General of Guatemala was **Rámses Cuestas Gómez**.'

- grounding (date-aware) — in store: **False** · in frames B saw: **False** · on pages: fdea9e43.md=False  · fuzz(gold, frames): 90

  - [ ] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [ ] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Attorney General of Guatemala",
        "inaugural holder"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "focus": [
        {
          "ref": "#12853",
          "name": "Attorney General of Guatemala",
          "kind": "public office",
          "summary": "Guatemala's chief public prosecutor and head of the Ministerio Público."
        },
        {
          "ref": "#12885",
          "name": "inaugural holder"
        }
      ],
      "recent": [
        {
          "ref": "rel:30266",
          "attrs": {
            "subject": "Attorney General of Guatemala",
            "inaugural holder": "Rámses Cuestas Gómez"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:30263",
          "attrs": {
            "subject": "Attorney General of Guatemala",
            "appointed by": "President of Guatemala"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:30298",
          "attrs": {
            "subject": "José Amílcar Velásquez Zárate",
            "held office": "Attorney General of Guatemala"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:30277",
          "attrs": {
            "subject": "Rámses Cuestas Gómez",
            "held office": "Attorney General of Guatemala"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:30275",
          "attrs": {
            "subject": "Attorney General of Guatemala",
            "removable by": "President of Guatemala"
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
          "ref": "rel:15000",
          "attrs": {
            "subject": "Pakistan People's Party",
            "founded in": "Lahore"
          },
          "status": "asserted",
          "confidence": 1,
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
- **arm B answer:** 'Manuleleua Paletasala Tovale was appointed on **28 July 2021**.'

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
      "history_depth": 2
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
              "ref": "#5827",
              "name": "appointed Finance Minister",
              "score": 0.78
            },
            {
              "ref": "#6894",
              "name": "nominated for prime minister",
              "score": 0.74
            },
            {
              "ref": "#11708",
              "name": "Chief of the Cabinet of Ministers",
              "score": 0.74
            },
            {
              "ref": "#5759",
              "name": "appointed defence secretary",
              "score": 0.73
            },
            {
              "ref": "#5861",
              "name": "supported as prime minister",
              "score": 0.71
            },
            {
              "ref": "#5032",
              "name": "appointed as",
              "score": 0.71
            },
            {
              "ref": "#5766",
              "name": "finance minister predecessor",
              "score": 0.71
            },
            {
              "ref": "#4957",
              "name": "cabinet appointment date",
              "score": 0.71
            },
            {
              "ref": "#2042",
              "name": "appointed to political secretariat",
              "score": 0.7
            },
            {
              "ref": "#6899",
              "name": "Prime Minister's Secretariat",
              "score": 0.7
            },
            {
              "ref": "#8819",
              "name": "Australian prime minister",
              "score": 0.7
            },
            {
              "ref": "#238",
              "name": "shadow minister predecessor",
              "score": 0.7
            },
            {
              "ref": "#239",
              "name": "shadow minister successor",
              "score": 0.7
            },
            {
              "ref": "#11535",
              "name": "appointed",
              "score": 0.7
            },
            {
              "ref": "#10562",
              "name": "cabinet",
              "score": 0.7
            },
            {
              "ref": "#5767",
              "name": "finance minister successor",
              "score": 0.69
            },
            {
              "ref": "#11509",
              "name": "cabinet chief",
              "score": 0.69
            },
            {
              "ref": "#10561",
              "name": "Asim_Ahmed_Khan — infobox, Cabinet Minister",
              "kind": "pointer",
              "score": 0.69
            },
            {
              "ref": "#6134",
              "name": "appointed to",
              "score": 0.69
            },
            {
              "ref": "#294",
              "name": "appointed by",
              "score": 0.69
            },
            {
              "ref": "#229",
              "name": "predecessor as Member of Parliament",
              "score": 0.68
            },
            {
              "ref": "#6776",
              "name": "proposed number of prime ministers",
              "score": 0.68
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#14603",
          "name": "Manuleleua Paletasala Tovale",
          "kind": "person",
          "summary": "Samoan politician and former member of the Legislative Assembly of Samoa."
        }
      ],
      "recent": [
        {
          "ref": "rel:35298",
          "attrs": {
            "subject": "Manuleleua Paletasala Tovale",
            "appointed as associate minister": "2021-07-28"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:35285",
          "attrs": {
            "subject": "Manuleleua Paletasala Tovale",
            "occupation": "politician"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:35299",
          "attrs": {
            "subject": "Manuleleua Paletasala Tovale",
            "dismissed as associate minister": "2025-01-17"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
     
```
</details>

### qid=4313 — FIXED  (A=incorrect → B=correct)  — ✓ grounded

- **Q:** Who was the mayor of Kathmandu who served from 1971 to 1976?
- **gold:** `Rajendra Man Suwal`
- **arm A answer:** 'Dhan Bahadur Shakya'
- **arm B answer:** 'Rajendra Man Suwal served as Kathmandu’s Pradhan Panch (mayoral equivalent) from 1971 to 1976.'

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
              "ref": "#14646",
              "name": "Kathmandu Municipal Executive",
              "kind": "government body",
              "score": 0.84
            },
            {
              "ref": "#14665",
              "name": "Rajaram Shrestha",
              "kind": "person",
              "score": 0.82
            },
            {
              "ref": "#14655",
              "name": "Janak Man Shrestha",
              "kind": "person",
              "score": 0.82
            },
            {
              "ref": "#14656",
              "name": "Prayagraj Singh Suwal",
              "kind": "person",
              "score": 0.81
            },
            {
              "ref": "#14666",
              "name": "Bidya Sundar Shakya",
              "kind": "person",
              "score": 0.8
            },
            {
              "ref": "#14641",
              "name": "Electorate of Kathmandu",
              "kind": "group",
              "score": 0.8
            },
            {
              "ref": "#14669",
              "name": "Chandra Shumsher government",
              "kind": "government",
              "score": 0.8
            },
            {
              "ref": "#14667",
              "name": "Balendra Shah",
              "kind": "person",
              "score": 0.79
            },
            {
              "ref": "#14663",
              "name": "Prem Lal Singh",
              "kind": "person",
              "score": 0.79
            },
            {
              "ref": "#14638",
              "name": "Kathmandu Metropolitan City",
              "kind": "municipality",
              "score": 0.78
            },
            {
              "ref": "#4775",
              "name": "mayor of",
              "score": 0.77
            },
            {
              "ref": "#14645",
              "name": "Kathmandu Metropolitan City Municipal Assembly",
              "kind": "government body",
              "score": 0.76
            },
            {
              "ref": "#14729",
              "name": "Kathmandu deputy chairman",
              "score": 0.76
            },
            {
              "ref": "#14637",
              "name": "Pradhan Panch of Kathmandu",
              "kind": "public office",
              "score": 0.74
            },
            {
              "ref": "#14727",
              "name": "Mayor Man",
              "score": 0.73
            },
            {
              "ref": "#14654",
              "name": "Shankar Dev Pant",
              "kind": "person",
              "score": 0.72
            },
            {
              "ref": "#14707",
              "name": "mayoral term",
              "score": 0.71
            },
            {
              "ref": "#4779",
              "name": "Taipei mayoral predecessor",
              "score": 0.71
            },
            {
              "ref": "#14653",
              "name": "Gehendra Shumsher Thapa",
              "kind": "person",
              "score": 0.71
            },
            {
              "ref": "#4780",
              "name": "Taipei mayoral successor",
              "score": 0.7
            },
            {
              "ref": "#5183",
              "name": "Huang Ta-chou",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#5239",
              "name": "appointed as Mayor of Taipei",
              "score": 0.69
            }
          ]
        },
        {
          "at": "focus[1]",
          "submitted": "1971 1976",
          "ref": "#14658",
          "name": "Rajendra Man Suwal",
          "via": "lexical",
          "score": 0.83
        }
      ],
      "focus": [
        {
          "ref": "#14658",
          "name": "Rajendra Man Suwal",
          "kind": "person",
          "summary": "Pradhan Panch of Kathmandu from 1971 to 1976."
        }
      ],
      "recent": [
        {
          "ref": "rel:35396",
          "attrs": {
            "subject": "Rajendra Man Suwal",
            "Pradhan Panch term": "1971–1976"
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

## BROKEN (3)

### qid=2790 — BROKEN  (A=correct → B=incorrect)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** What did the artist Lygia Pape initially study in university?
- **gold:** `Philosophy`
- **arm A answer:** 'Philosophy.'
- **arm B answer:** 'Lygia Pape initially studied **Moral Sciences** at the Federal University of Rio de Janeiro.'

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
        "university studies"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[1]",
          "submitted": "university studies",
          "resolved": false,
          "candidates": [
            {
              "ref": "#3528",
              "name": "Slade School of Fine Art",
              "kind": "organization",
              "score": 0.88
            },
            {
              "ref": "#4355",
              "name": "Trinity College, Oxford",
              "kind": "organization",
              "score": 0.88
            },
            {
              "ref": "#4356",
              "name": "University of Oxford",
              "kind": "organization",
              "score": 0.88
            },
            {
              "ref": "#6005",
              "name": "Budapest University of Technology and Economics",
              "kind": "organization",
              "score": 0.88
            },
            {
              "ref": "#10375",
              "name": "Imperial University of Dorpat",
              "kind": "organization",
              "score": 0.88
            },
            {
              "ref": "#10614",
              "name": "Federal University of Rio de Janeiro",
              "kind": "organization",
              "score": 0.88
            },
            {
              "ref": "#12177",
              "name": "Massachusetts Institute of Technology",
              "kind": "organization",
              "score": 0.88
            },
            {
              "ref": "#4802",
              "name": "Taihoku Higher School",
              "kind": "organization",
              "score": 0.69
            },
            {
              "ref": "#3876",
              "name": "studied in",
              "score": 0.73
            },
            {
              "ref": "#3324",
              "name": "study",
              "score": 0.73
            },
            {
              "ref": "#3564",
              "name": "studied at",
              "score": 0.73
            },
            {
              "ref": "#4060",
              "name": "field of study",
              "score": 0.72
            },
            {
              "ref": "#10648",
              "name": "studied with",
              "score": 0.72
            },
            {
              "ref": "#4686",
              "name": "studied",
              "score": 0.71
            },
            {
              "ref": "#4850",
              "name": "academic interest",
              "score": 0.7
            },
            {
              "ref": "#10646",
              "name": "studied subject",
              "score": 0.7
            },
            {
              "ref": "#3529",
              "name": "University College London",
              "kind": "organization",
              "score": 0.69
            },
            {
              "ref": "#1861",
              "name": "funded study of",
              "score": 0.69
            },
            {
              "ref": "#3824",
              "name": "University of Gloucestershire",
              "kind": "organization",
              "score": 0.68
            },
            {
              "ref": "#8402",
              "name": "study period",
              "score": 0.68
            },
            {
              "ref": "#7787",
              "name": "University of Chicago",
              "kind": "organization",
              "score": 0.67
            },
            {
              "ref": "#8168",
              "name": "study period end",
              "score": 0.66
            },
            {
              "ref": "#12976",
              "name": "university attendance duration",
              "score": 0.66
            },
            {
              "ref": "#3823",
              "name": "University for the Creative Arts",
              "kind": "organization",
              "score": 0.66
            },
            {
              "ref": "#7076",
              "name": "Oxford degree program",
              "score": 0.66
            },
            {
              "ref": "#10779",
              "name": "academic degree",
              "score": 0.65
            },
            {
              "ref": "#13857",
              "name": "campus includes",
              "score": 0.65
            },
            {
              "ref": "#10815",
              "name": "University of Bonn",
              "kind": "organization",
              "score": 0.64
            },
            {
              "ref": "#10852",
              "name": "chancellor's professor at",
              "score": 0.64
            },
            {
              "ref": "#14586",
              "name": "academic rank",
              "score": 0.64
            }
          ]
        }
      ],
      "focus": [
        {
          "ref": "#10611",
          "name": "Lygia Pape",
          "kind": "person",
          "summary": "Brazilian visual artist, sculptor, engraver, and filmmaker associated with Concrete and Neo-Concrete art."
        }
      ],
      "recent": [
        {
          "ref": "rel:24200",
          "attrs": {
            "subject": "Lygia Pape",
            "studied subject": "Moral Sciences"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24201",
          "attrs": {
            "subject": "Lygia Pape",
            "studied at": "Federal University of Rio de Janeiro"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "2024-07-03"
        },
        {
          "ref": "rel:24202",
          "attrs": {
            "subject": "Lygia Pape",
            "studied with": "Fayga Ostrower"
          },
          "status": "asserted",
          "confidence": 1,
          "support_count": 1,
          "date": "
```
</details>

### qid=3716 — BROKEN  (A=correct → B=incorrect)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** What was the prize money, in British pounds, awarded to the loser of the chess match featuring Wilhelm Steinitz and Adolf Anderssen in 1866?
- **gold:** `£20`
- **arm A answer:** '£20'
- **arm B answer:** 'Adolf Anderssen, the loser, received **£0**; Wilhelm Steinitz won the **£100** stake.'

- grounding (date-aware) — in store: **True** · in frames B saw: **False** · on pages: 99d31bd1.md=True  · fuzz(gold, frames): 67

  - [x] gold fact is **in the store**
  - [ ] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Wilhelm Steinitz",
        "Adolf Anderssen",
        "1866 chess match",
        "prize money loser"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[2]",
          "submitted": "1866 chess match",
          "resolved": false,
          "candidates": [
            {
              "ref": "#12952",
              "name": "Steinitz–Bird match",
              "kind": "event",
              "score": 1
            },
            {
              "ref": "#12951",
              "name": "Steinitz–Anderssen match",
              "kind": "event",
              "score": 0.93
            },
            {
              "ref": "#12943",
              "name": "Adolf Anderssen",
              "kind": "person",
              "score": 0.86
            },
            {
              "ref": "#12944",
              "name": "Henry Bird",
              "kind": "person",
              "score": 0.86
            },
            {
              "ref": "#13247",
              "name": "Henry Edward Bird",
              "kind": "person",
              "score": 0.86
            },
            {
              "ref": "#12939",
              "name": "Serafino Dubois",
              "kind": "person",
              "score": 0.79
            },
            {
              "ref": "#12950",
              "name": "Steinitz–Dubois match",
              "kind": "event",
              "score": 0.79
            },
            {
              "ref": "#12941",
              "name": "Frederick Deacon",
              "kind": "person",
              "score": 0.71
            },
            {
              "ref": "#13199",
              "name": "London handicap tournament 1866",
              "kind": "event",
              "score": 0.84
            },
            {
              "ref": "#13013",
              "name": "1866",
              "score": 0.83
            },
            {
              "ref": "#13281",
              "name": "Anderssen–Steinitz game 13 (London 1866)",
              "kind": "event",
              "score": 0.73
            },
            {
              "ref": "#12953",
              "name": "Steinitz–Zukertort match",
              "kind": "event",
              "score": 0.73
            },
            {
              "ref": "#13136",
              "name": "Steinitz–Chigorin telegraph match (1890)",
              "kind": "event",
              "score": 0.73
            },
            {
              "ref": "#12949",
              "name": "London 1862 chess tournament",
              "kind": "competition",
              "score": 0.73
            },
            {
              "ref": "#13197",
              "name": "London championship 1862",
              "kind": "event",
              "score": 0.71
            },
            {
              "ref": "#13091",
              "name": "Steinitz–Schiffers match 1896",
              "kind": "event",
              "score": 0.71
            },
            {
              "ref": "#12957",
              "name": "London 1872 chess tournament",
              "kind": "competition",
              "score": 0.71
            },
            {
              "ref": "#13086",
              "name": "Hastings 1895 chess tournament",
              "kind": "event",
              "score": 0.71
            },
            {
              "ref": "#13244",
              "name": "Eduard Jenay",
              "kind": "person",
              "score": 0.71
            },
            {
              "ref": "#13094",
              "name": "Morphy–Anderssen match 1858",
              "kind": "event",
              "score": 0.7
            },
            {
              "ref": "#12942",
              "name": "Valentine Green",
              "kind": "person",
              "score": 0.7
            },
            {
              "ref": "#13012",
              "name": "defeated in 1864 match",
              "score": 0.7
            },
            {
              "ref": "#13245",
              "name": "Lang",
              "kind": "person",
              "score": 0.69
            },
            {
              "ref": "#13200",
              "name": "Dundee handicap tournament 1867",
              "kind": "event",
              "score": 0.69
            },
            {
              "ref": "#12958",
              "name": "Vienna 1873 chess tournament",
              "kind": "competition",
              "score": 0.69
            },
            {
              "ref": "#13255",
              "name": "Steinitz–Jenay match (1860)",
              "kind": "event",
              "score": 0.69
            },
            {
              "ref": "#13087",
              "name": "Saint Petersburg chess tournament 1895",
              "kind": "event",
              "score": 0.68
            },
            {
              "ref": "#12995",
              "name": "American Chess Congress cooperation period",
              "score": 0.68
            },
            {
              "ref": "#13092",
              "name": "World Chess Championship 1896–1897",
              "kind": "event",
              "score": 0.68
            },
            {
              "ref": "#13002",
              "name": "Vienna City Championship 1861 place",
              "score": 0.68
            }
          ]
        },
        {
          "at": "focus[3]",
          "submitted": "prize money loser",
          "resolved": false,
          "candidates": [
            {
              "ref": "#13243",
              "name": "Steinitz prize money",
              "score": 0.6
            },
            {
              "ref": "#5170",
              "name": "Lee Teng-hui graft indictment",
              "kind": "event",
              "score": 0.4
            },
            {
              "ref": "#2002",
              "name": "Bulgaria",
              "kind": "place",
              "score": 0.33
            },
            {
              
```
</details>

### qid=4042 — BROKEN  (A=correct → B=incorrect)  — ✗ NOT grounded (gold not in a frame B saw)

- **Q:** What was the last name of the senator from New York who presented Reverend Billy Graham with the Gold Award of the George Washington Carver Memorial Institute in 1964?
- **gold:** `Javits `
- **arm A answer:** 'Javits'
- **arm B answer:** 'Keating. Senator **Kenneth Keating** presented the award.'

- grounding (date-aware) — in store: **False** · in frames B saw: **True** · on pages: d36b1897.md=True  · fuzz(gold, frames): 100

  - [ ] gold fact is **in the store**
  - [x] gold was **in a frame Terra actually saw**
  - [x] gold is **on a snapshot page**

<details><summary>recall frames arm B saw</summary>

```json
[
  {
    "args": {
      "focus": [
        "Reverend Billy Graham",
        "Gold Award",
        "George Washington Carver Memorial Institute",
        "1964",
        "senator New York"
      ],
      "limit": 5,
      "history_depth": 0
    },
    "frame": {
      "resolution": [
        {
          "at": "focus[0]",
          "submitted": "Reverend Billy Graham",
          "resolved": false,
          "candidates": [
            {
              "ref": "#14309",
              "name": "1972 Nixon–Graham conversation",
              "kind": "event",
              "score": 0.74
            },
            {
              "ref": "#7167",
              "name": "Mitt Romney",
              "kind": "person",
              "score": 0.68
            },
            {
              "ref": "#12270",
              "name": "Richard Nixon",
              "kind": "person",
              "score": 0.68
            },
            {
              "ref": "#13900",
              "name": "Robert Schuller",
              "kind": "person",
              "score": 0.68
            },
            {
              "ref": "#13902",
              "name": "Sharon Grammar School",
              "kind": "organization",
              "score": 0.68
            },
            {
              "ref": "#13904",
              "name": "Mordecai Ham",
              "kind": "person",
              "score": 0.68
            },
            {
              "ref": "#13905",
              "name": "Bob Jones College",
              "kind": "organization",
              "score": 0.68
            },
            {
              "ref": "#13906",
              "name": "Florida Bible Institute",
              "kind": "organization",
              "score": 0.68
            },
            {
              "ref": "#13882",
              "name": "Billy Graham",
              "kind": "person",
              "score": 0.83
            },
            {
              "ref": "#14225",
              "name": "Billy_Graham: Pastor to presidents",
              "kind": "pointer",
              "score": 0.8
            },
            {
              "ref": "#14234",
              "name": "criticized Billy Graham in",
              "score": 0.78
            },
            {
              "ref": "#14543",
              "name": "Evangelist to the World",
              "kind": "work",
              "score": 0.77
            },
            {
              "ref": "#14546",
              "name": "A Biblical Standard for Evangelists",
              "kind": "work",
              "score": 0.77
            },
            {
              "ref": "#14429",
              "name": "Billy Graham papers",
              "score": 0.76
            },
            {
              "ref": "#14281",
              "name": "Billy_Graham: presidential funerals",
              "kind": "pointer",
              "score": 0.76
            },
            {
              "ref": "#14220",
              "name": "Billy_Graham: claim disputed by other descendants",
              "kind": "pointer",
              "score": 0.76
            },
            {
              "ref": "#14413",
              "name": "presented to Billy Graham by",
              "score": 0.76
            },
            {
              "ref": "#14459",
              "name": "Anglican Church in North America",
              "kind": "organization",
              "score": 0.76
            },
            {
              "ref": "#14275",
              "name": "Billy_Graham: 1970 revival",
              "kind": "pointer",
              "score": 0.76
            },
            {
              "ref": "#13908",
              "name": "Peniel Baptist Church",
              "kind": "organization",
              "score": 0.75
            },
            {
              "ref": "#13887",
              "name": "Melvin Thomas Graham",
              "kind": "person",
              "score": 0.75
            },
            {
              "ref": "#14217",
              "name": "reported 2016 vote by Billy Graham for",
              "score": 0.74
            },
            {
              "ref": "#14535",
              "name": "Peace with God",
              "kind": "work",
              "score": 0.74
            },
            {
              "ref": "#14375",
              "name": "Just As I Am",
              "kind": "work",
              "score": 0.74
            },
            {
              "ref": "#14540",
              "name": "World Aflame",
              "kind": "work",
              "score": 0.74
            },
            {
              "ref": "#13883",
              "name": "William Franklin Graham Sr.",
              "kind": "person",
              "score": 0.74
            },
            {
              "ref": "#14394",
              "name": "Thank You Billy Graham",
              "kind": "work",
              "score": 0.74
            },
            {
              "ref": "#13890",
              "name": "Franklin Graham",
              "kind": "person",
              "score": 0.74
            },
            {
              "ref": "#14547",
              "name": "Unto the Hills",
              "kind": "work",
              "score": 0.74
            },
            {
              "ref": "#13894",
              "name": "Billy Graham Evangelistic Association",
              "kind": "organization",
              "score": 0.74
            }
          ]
        },
        {
          "at": "focus[1]",
          "submitted": "Gold Award",
          "resolved": false,
          "candidates": [
            {
              "ref": "#14477",
              "name": "George Washington Carver Gold Award",
              "score": 1
            },
            {
              "ref": "#696",
              "name": "Royal Philharmonic Society Gold Medal",
              "kind": "award",
              "score": 0.88
            },
            {
              "ref": "#12514",
              "name": "Charles X of France",
              "kind": "person",
     
```
</details>
