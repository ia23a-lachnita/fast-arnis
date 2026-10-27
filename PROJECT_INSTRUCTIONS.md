# PROJECT INSTRUCTIONS — fast-arnis

## Standing Rules

### 1. Never End Turn Before Verification
- Build, test, and run processes must reach a decisive state (pass/fail/complete) before ending a turn.
- Always verify with tool output, never assume or state "started" without completion.
- When reporting working state, include actual verification output.

### 2. Never Stop at Error Without Reporting and Fixing
- If an error occurs, report the exact error immediately.
- If fixable: fix, verify, end with `fixed: [what]`.
- If unfixable: state the exact failure, end with `failed: [reason] — not fixed`.
- Never end turn with "maybe" or "will check".

### 3. Empirical Beats Theoretical
- When multiple optimization options exist, do not speculate.
- Benchmark before and after every optimization step.
- The test and profile decide, not theory.

---

## Project Purpose & Scope: fast-arnis

Fast-Arnis is an optimized, high-throughput fork/enhancement of Arnis (Minecraft real-world generator) designed to fully saturate modern hardware (CPU multi-threading, high-capacity RAM caching, GPU acceleration, fast I/O).

### Target Hardware (Host Specs)
- CPU: Intel Core i7-10700K (8C / 16T @ 3.80GHz - 5.1GHz)
- GPU: NVIDIA GeForce RTX 3070 (8GB GDDR6 VRAM, CUDA 13.3 capable)
- RAM: 32 GB DDR4 (large in-memory working sets & cache)

### Core Objectives
1. **Full Hardware Saturation**:
   - Eliminate arbitrary thread limits (e.g. 90% rayon cap) and enable adaptive full-core threading.
   - Utilize high-RAM caching (pipelined chunk generation, batch memory buffering before disk flush).
   - Offload heavy compute tasks (coordinate transforms, heightmap/DEM interpolations, rasterization, flood-fill masks) to SIMD and GPU (wgpu/compute shaders or CUDA) where beneficial.
2. **Dynamic ETA & Progress Reporting**:
   - Provide real-time generation speed (chunks/s, tiles/s, blocks/s), elapsed time, stage-level progress, and accurate ETA estimates in both CLI and GUI.
3. **Hardware Capacity Detection & Soft World Limits Warning**:
   - Auto-detect available CPU, RAM, and GPU capabilities.
   - Compute estimated memory and processing requirements for selected bounding box/world size.
   - Warn user before generation if selected area exceeds detected hardware limits, with recommended chunking/tiling strategies.
4. **End Goal — Switzerland Scale**:
   - Enable massive-scale world generation (e.g. entire Switzerland bounding box: ~348 km x 220 km) via tiled streaming, low-overhead Anvil chunk serialization, and memory-bounded pipelines without OOM or crashing.
