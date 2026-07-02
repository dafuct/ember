# CMake toolchain shim for portable Apple-Silicon builds.
#
# whisper-rs-sys builds whisper.cpp/ggml with GGML_NATIVE=ON by default, which uses
# `-mcpu=native` and auto-enables per-CPU features (dotprod, i8mm, sve). That is wrong
# for a *distributed* binary: it bakes the BUILD machine's instruction set into the app.
# Base Apple M1 lacks FEAT_I8MM, so a native build made on an M2+/M3+ Mac emits i8mm
# instructions (e.g. vmmlaq_s32 / smmla) that SIGILL-crash on an M1. It also breaks CI,
# where ggml's native feature detection produces an inconsistent flag state and fails to
# compile ggml-cpu-quants.c.
#
# Disabling GGML_NATIVE makes ggml build a conservative arm64 baseline (no i8mm), so the
# resulting .dmg runs on every Apple-Silicon Mac (M1 and up). We inject this file via the
# CMAKE_PROJECT_INCLUDE env var (whisper-rs-sys forwards any CMAKE_* var to cmake). CMake
# runs it right after the top-level project() call — before ggml's option(GGML_NATIVE) —
# so FORCE-setting the cache here wins (option() never overrides an existing cache entry).
# (CMAKE_TOOLCHAIN_FILE does NOT work: the cmake-rs crate overrides it on native builds.)
set(GGML_NATIVE OFF CACHE BOOL "portable arm64 baseline for distribution" FORCE)
