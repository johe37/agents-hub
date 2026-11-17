SEPARATORS = "/-_. "


def _token_score(token: str, text: str) -> int | None:
    i = text.find(token)
    if i >= 0:
        score = 100 + 10 * len(token)
        if i == 0 or text[i - 1] in SEPARATORS:
            score += 50
        if i > text.rfind("/"):
            score += 40
        return score
    # Fall back to an in-order subsequence match, like fzf.
    score, pos, prev = 0, -1, -2
    for ch in token:
        pos = text.find(ch, pos + 1)
        if pos < 0:
            return None
        if pos == prev + 1:
            score += 8
        elif pos == 0 or text[pos - 1] in SEPARATORS:
            score += 6
        else:
            score += 1
        prev = pos
    return score


def score(query: str, text: str) -> int | None:
    """Score text against a query; None means no match. Every word must match."""
    text = text.lower()
    total = 0
    for token in query.lower().split():
        s = _token_score(token, text)
        if s is None:
            return None
        total += s
    return total


def rank(query: str, items: list, key) -> list:
    if not query.strip():
        return list(items)
    scored = [(score(query, key(it)), it) for it in items]
    matched = [(s, it) for s, it in scored if s is not None]
    matched.sort(key=lambda pair: (-pair[0], len(key(pair[1]))))
    return [it for _, it in matched]
