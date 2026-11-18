# C and C++

wasi-sdk plus the C backend of `wit-bindgen`. `--language c` and `--language cpp` share
everything here; the differences are in the last section.

```shell
nix develop .#component-c           # wasi-sdk 27, wit-bindgen, wasm-tools, cmake, ninja

wit-bindgen c wit --world lm75a --out-dir bindings
clang --target=wasm32-wasip2 -O2 -I. -c lm75a.c -o build/lm75a.o
clang --target=wasm32-wasip2 -O2 -I. -c bindings/lm75a.c -o build/bindings.o
clang --target=wasm32-wasip2 -mexec-model=reactor \
    build/lm75a.o build/bindings.o bindings/lm75a_component_type.o -o build/lm75a.wasm
```

`build-component.sh` runs those commands. Three of their details are worth knowing,
because each fails in a way a compiler will not report.

## The link is a reactor, not a command

`-mexec-model=reactor` says the exports are the program. With the default command model
the linker wants `_start`, and the result runs once and exits, which NEST cannot use: a
component is never called, and nothing complains.

## The component type section comes from the bindings

`bindings/<world>_component_type.o` carries the `component-type` section, and linking it
is what makes the output a component rather than a core module — there is no
`wasm-tools component new` step in this route. Check with
`wasm-tools component wit build/<name>.wasm`; a core module is the usual sign that the
object was left out.

## The generated names come from the world

Files and symbols are named after the world, dashes become underscores, and every type and
function carries that prefix: the world `example:components/imu-test` writes
`bindings/imu_test.h`, `imu_test_string_t`, `imu_test_i2c_transaction_t`. The header holds
one `extern void <world>_<import>(...)` per import — `imu_test_i2c_out`, `imu_test_emit` —
and one `<world>_<export>(...)` per export, prefixed `exports_`.

Variants are a tag and a union, which is the part written by hand most often:

```c
switch (msg->tag) {
case NEST_PLUGIN_TYPES_I2C_TRANSACTION_REQUEST: {
    nest_plugin_types_i2c_request_t *req = &msg->val.request;
    ...
}
case NEST_PLUGIN_TYPES_I2C_TRANSACTION_RESPONSE: ...
}
```

Results are a `bool` return with an out parameter for the error:

```c
bool exports_imu_test_init(imu_test_list_tuple_string_string_t *options,
                           imu_test_init_error_t *err);
```

so refusing to start means setting `err->tag` to `UNKNOWN_OPTION` or `INVALID_VALUE`,
filling its `val` string, and returning `false`. For `command_error`, `UNKNOWN_COMMAND` has
no payload — the tag alone is the answer, and the host logs the command name.

## Memory, and who frees what

Strings are `{ char *ptr; size_t len; }`, and are *not* NUL-terminated coming from the
host. The component-model rule:

- arguments handed to an export belong to the caller; copy whatever you keep
- values passed to an import may be read by the host until the call returns, so do not free
  them before it and do not keep the pointer after
- the `_free` functions release what the *bindings* allocated, mostly after a
  `<world>_string_dup`

The failure mode is a component that answers one transaction correctly and then answers with
garbage, having kept a pointer into a buffer the host reused.

## WIT to C, where it surprises

| WIT | C |
|---|---|
| `string` | struct with `ptr` and `len`, not NUL-terminated |
| `bytes`, `list<u8>` | struct with `ptr` and `len` |
| `tuple<a, b>` | struct with `f0`, `f1` |
| `option<t>` | struct with `is_some` and a `val` union |
| `variant` | struct with `tag` and a `val` union, plus `#define`s for the tags |
| `result<ok, err>` | `bool` return plus an out parameter |
| `enum` | `uint8_t` tag with `#define`s |

## CMake

```shell
cmake -B build -DCMAKE_TOOLCHAIN_FILE=$WASI_SDK_PATH/share/cmake/wasi-sdk-p2.cmake
```

That file sets the triple and the reactor link model, which is what a project with several
sources wants. The scaffold does not use CMake: one `.c` file needs no build system.

## C++

The same toolchain and the same bindings, with three differences that each fail
confusingly if guessed at:

- **compile the bindings as C.** `wit-bindgen c` writes C, and the component type object is
  referenced through a C symbol. Give `bindings/<world>.c` to `clang++` and the link ends in
  `undefined symbol: __component_type_object_force_link_<world>()`. `build-component.sh`
  compiles that file with `clang`, the source with `clang++`, and links with `clang++`.
- **cast string pointers.** C converts `uint8_t *` to `char *` without comment; C++ does
  not, so `strncmp(command->ptr, "reset", 5)` will not compile. Write
  `strncmp((const char *) command->ptr, "reset", 5)`, which is also valid C.
- **add `-std=c++20`**, because the generated structs use designated initializers.

Everything else on this page applies unchanged.
