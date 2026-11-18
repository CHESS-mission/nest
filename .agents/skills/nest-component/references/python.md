# Python

componentize-py, with WASI stubbed at build time.

```shell
nix develop .#component-python              # uv, wasm-tools

uv run --python 3.12 --with componentize-py \
    componentize-py -d wit -w lm75a componentize app --stub-wasi -o build/lm75a.wasm
```

`build-component.sh` runs that. `--python 3.12` is the version componentize-py supports; uv
fetches the interpreter on first use, so the first build is slower than the rest.

## `--stub-wasi` is required, not optional

CPython looks at `wasi:cli/environment` while starting, before any of your code runs, and
the host does not implement it. Without the flag the component traps before `init`, and the
traceback names a file that does not exist. With it, the build answers those imports itself
and the artifact imports only `nest:plugin` and the world's ports.

The consequence to know: WASI is unavailable *by construction*. Anything in the standard
library that reaches for the operating system — files, environment, subprocesses, sockets —
either fails at build time or becomes a stub that traps. Keep imports at the top of `app.py`,
where the build can see them: an import inside a handler runs at call time, and that is a
host call.

## State is module level

A fresh `WitWorld` instance is constructed for every call, so `self.registers = [...]` in
`init` is gone by the time a transaction arrives. Keep the state in a module-level object:

```python
state = {
    "address": None,
    "pointer": 0,
    "registers": [0] * 8,
}
```

## Nothing may raise out of a handler

Handling an exception means printing a traceback, and printing one imports `os` and stats
the filesystem. That reaches a stub, and the trap replaces the message that would have
explained it. Wrap every body, report with `log.error`, and let only deliberate errors
escape:

```python
def on_command(self, command, args):
    try:
        ...
        raise Err(errors.CommandError_UnknownCommand())     # a result error: meant to escape
    except Err:
        raise
    except BaseException as error:
        log.error(f"command {command} failed: {error!r}")
        raise Err(errors.CommandError_InvalidValue(value=str(error)))
```

`Err` comes from `componentize_py_types` and is how a `result<_, e>` returns its error case;
the classes in `wit_world.imports.errors` are the variants.

## Naming

`WitWorld` in `wit_world.py` is what the tool expects; the class is the world, and its
methods are the exports, named as in WIT — `init`, `on_command`, `i2c_in`, `on_timeout`.
Imports are module-level functions in `wit_world`: `emit`, `get_time`, `set_timer`,
`i2c_out`. Importing a function the world does not declare fails the build, which is the one
check in this language that is not silent.

## Types

| WIT | Python |
|---|---|
| `string` | `str` |
| `bytes`, `list<u8>` | `bytes` |
| `list<T>` | `list[T]` |
| `tuple<a, b>` | `tuple[a, b]` |
| `record` | a class of the same name, keyword arguments |
| `variant` | one class per case, e.g. `I2cTransaction_Request` with `.value` |
| `option<t>` | `t` or `None` |
| `result<ok, err>` | `ok`, or `raise Err(...)` |
| `enum` | class attribute of the enum |

`variant` cases are classes rather than tags, so a handler dispatches with `isinstance`.

## Time and randomness

`time.time()` reads a frozen clock: the interpreter's view of time is from the build, not
the run. Use `get_time()` for elapsed seconds and `set_timer(ms)` with `on_timeout` for
periodic work. `random` behaves the same way — seeded at build time, repeating its sequence
in every run — so a model that needs noise should take it from an environment port or from
a command argument rather than from the generator.

## Reference material

- componentize-py: <https://github.com/bytecodealliance/componentize-py>
