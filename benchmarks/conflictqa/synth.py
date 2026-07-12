"""Synthetic-entity corpus for the conflict_resolution benchmark.

The famous-entity variant is confounded: a frontier consumer overrides retrieved
counterfactuals with world knowledge (it answers "Charles Dickens" even when the
store says the author is someone else). Synthetic entities remove that prior — the
consumer has never heard of "Vornex Qualdaris", so the ONLY way to answer is the
store, and arm A (no store) correctly fails.

generate() is deterministic (seeded, no I/O). It builds N clusters, each a fully
linked chain (work -> author -> spouse -> country/city; author -> sport -> country
-> capital), injects latest-wins conflicts on a subset of edges, and emits single-
and multi-hop questions whose gold is the post-conflict (current) value.
"""

import random

ONSETS = ["Br", "Dr", "Gl", "Kr", "Th", "Vr", "Zn", "Ql", "Sk", "Pl", "Tr",
          "My", "Xa", "Ph", "Sh", "Fr", "Gw", "Vor", "Zel", "Cad"]
VOWELS = ["a", "e", "i", "o", "u", "y", "ae", "oo", "ei"]
CODAS = ["x", "n", "k", "th", "l", "rn", "sk", "m", "v", "dor", "vale",
         "mor", "eth", "ux", "gar", "wen", "dris", "phel", "tok", "zra"]


class Namer:
    def __init__(self, seed):
        self.r = random.Random(seed)
        self.used = set()

    def _tok(self):
        return self.r.choice(ONSETS) + self.r.choice(VOWELS) + self.r.choice(CODAS)

    def _uniq(self, make):
        while True:
            n = make()
            if n not in self.used:
                self.used.add(n)
                return n

    def person(self):
        return self._uniq(lambda: f"{self._tok().capitalize()} {self._tok().capitalize()}")

    def place(self):
        return self._uniq(lambda: self._tok().capitalize())

    def sport(self):
        return self._uniq(lambda: self._tok().lower())

    def work(self):
        return self._uniq(lambda: f"The {self._tok().capitalize()} {self._tok().capitalize()}")


# property name -> (question phrasing uses these verbatim; kept human-readable so
# recall frames are legible)
P_AUTHOR, P_SPOUSE, P_CITIZEN, P_DIED, P_SPORT, P_INVENTED, P_CAPITAL = (
    "authored by", "spouse", "citizen of", "died in city",
    "plays sport", "invented in country", "capital")


def generate(seed=7, n_clusters=6):
    nm = Namer(seed)
    r = random.Random(seed * 31 + 1)
    elements, base_facts, changes, questions = [], [], [], []

    def el(name, kind):
        elements.append({"name": name, "kind": kind})
        return name

    def fact(s, p, o):
        base_facts.append({"s": s, "p": p, "o": o})

    def conflicted(s, p, gold, kind_of_o):
        # assert a superseded value, then update to gold (latest-wins)
        old = {"person": nm.person, "place": nm.place, "sport": nm.sport}[kind_of_o]()
        el(old, kind_of_o if kind_of_o != "place" else "place")
        fact(s, p, old)
        changes.append({"target": s, "property": p, "from": old, "to": gold})

    for i in range(n_clusters):
        work = el(nm.work(), "work")
        author = el(nm.person(), "person")
        spouse = el(nm.person(), "person")
        sport = el(nm.sport(), "sport")
        c_citizen = el(nm.place(), "place")     # spouse's country
        c_sport = el(nm.place(), "place")        # sport's origin country
        death_city = el(nm.place(), "place")
        cap_citizen = el(nm.place(), "place")    # capital of c_citizen
        cap_sport = el(nm.place(), "place")      # capital of c_sport

        # edges — conflict on author, sport origin, and sport-country capital
        conflicted(work, P_AUTHOR, author, "person")
        fact(author, P_SPOUSE, spouse)
        fact(spouse, P_CITIZEN, c_citizen)
        fact(spouse, P_DIED, death_city)
        fact(author, P_SPORT, sport)
        conflicted(sport, P_INVENTED, c_sport, "place")
        fact(c_citizen, P_CAPITAL, cap_citizen)
        conflicted(c_sport, P_CAPITAL, cap_sport, "place")

        qs = [
            ("single", f"Who is the author of {work}?", author),
            ("single", f"What sport does {author} play?", sport),
            ("single", f"What is the capital of {c_sport}?", cap_sport),
            ("multi", f"In which city did the spouse of {author} die?", death_city),
            ("multi", f"What is the country of citizenship of the spouse of the author of {work}?", c_citizen),
            ("multi", f"What is the capital of the country in which the sport played by {author} was invented?", cap_sport),
        ]
        for hop, q, gold in qs:
            questions.append({"qid": f"{hop}_{len(questions)}", "hop": hop, "q": q, "gold": [gold]})

    r.shuffle(questions)
    return {"elements": elements, "base_facts": base_facts,
            "changes": changes, "questions": questions}


if __name__ == "__main__":
    d = generate()
    print(f"elements={len(d['elements'])} base_facts={len(d['base_facts'])} "
          f"changes={len(d['changes'])} questions={len(d['questions'])}")
    for q in d["questions"][:6]:
        print(f"  [{q['hop']:6}] {q['q']}  -> {q['gold']}")
    print("sample changes:", d["changes"][:2])
