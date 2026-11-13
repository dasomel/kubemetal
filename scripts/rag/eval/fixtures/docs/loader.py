# Resolve CLI paths before spawning children: desktop apps inherit no shell PATH.
def resolve_cli_path(name, search_paths):
    for directory in search_paths:
        candidate = directory / name
        if candidate.is_file():
            return candidate
    raise FileNotFoundError(name)
