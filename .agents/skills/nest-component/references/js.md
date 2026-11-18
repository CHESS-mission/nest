# JavaScript

componentize-js, with every WASI feature disabled. The result is a component that imports
nothing but `nest:plugin` and its own ports, which is what this host requires.

```shell
nix develop .#component-js          # node 22, wasm-tools, uv

npm install --no-save @bytecodealliance/componentize-js     # once, into the project
./node_modules/.bin/jco componentize app.js --wit wit \
    --world-name lm75a -d all -o build/lm75a.wasm
```

`build-component.sh` runs those, fetching the package only if `node_modules/.bin/jco` is
missing. Note that `-w` is `--wit`, not the world name; the world is `--world-name`.

## `-d all` is the requirement

The engine embedded in the output runs startup code — allocation, garbage collection, the
realm — and startup touches WASI. With features left enabled the component imports
`wasi:cli/environment` and friends, and dies before `init` is reached. `-d all` disables
every feature, and the component starts.

The cost is a module of some megabytes and a boot of a couple of seconds, which is the
engine rather than your code. `verify.sh` waits for initialization rather than assuming it
is instant.

## Imports

An interface import is a module:

```javascript
import { info } from 'nest:plugin/log';
```

A function imported at the *root* of the world — which is what a NEST port is — is a
default export named after the function:

```javascript
import emit from 'emit';
import getTime from 'get-time';
import i2cOut from 'i2c-out';
```

That asymmetry is the most common mistake in this route: `import { emit } from 'emit'`
fails with an undefined binding rather than a clear error.

## Exports

Top-level functions in `app.js` become the world's exports, matched by name: `init`,
`onCommand` for `on-command`, `<port>In` for `<port>-in`, `onTimeout` for `on-timeout`.
State can be an ordinary module-level variable; the module is evaluated once and holds for
the life of the simulation.

## Errors

Return values follow the WIT. For a `result`, the error case is a variant object, and
throwing it is how you return one:

```javascript
throw { tag: 'unknown-command' };
throw { tag: 'invalid-value', val: 'i2c_address must be a number' };
```

which reaches the log as `Unknown command: reset` or `Invalid value: …`.

A plain `throw new Error(...)` is different: the engine prints a traceback, and a traceback
touches the filesystem and the environment, which the host does not implement, so the call
traps and the instance is poisoned. Keep exceptions inside the handler that caught them, and
let the only escape be a variant.

## Types

| WIT | JavaScript |
|---|---|
| `string` | `string` |
| `bytes`, `list<u8>` | `Uint8Array` |
| `list<T>` | `Array<T>` |
| `tuple<a, b>` | `[a, b]` |
| `record` | plain object with the record's fields |
| `variant` | `{ tag, val }` |
| `option<t>` | `t` or `undefined` |
| `result<ok, err>` | `ok`, or throw `{ tag, val }` |
| `enum` | string equal to the case name |
| `u32`, `i32`, `f32`, `f64` | `number` |
| `u64`, `s64` | `bigint` |

## Time

`Date.now()` reads a WASI clock and traps; so does `setTimeout`, which is a host timer the
component model does not provide. Use `get-time` for the elapsed time in seconds and
`set-timer` with `on-timeout` for anything periodic.

## Reference material

- componentize-js: <https://github.com/bytecodealliance/componentize-js>
- `jco`, the CLI used above: <https://github.com/bytecodealliance/jco>
