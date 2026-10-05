# ADR-0001: Use a local modular monolith

## Status
Proposed

## Context
FlowDictate is a single-user desktop application with latency-sensitive in-process audio and strict offline/privacy constraints. Service boundaries would add serialization, IPC, deployment, and attack surface without scale benefit.

## Decision
Use one local process initially, organized as narrow Rust crates and worker domains. Permit process isolation later only for a measured containment need such as high-risk media import or model-runtime isolation.

## Consequences

- Positive: least operational complexity, no service/network fabric, direct bounded queues, small audit surface.
- Negative: a memory-safety or native-FFI flaw can affect the whole process; a process crash interrupts dictation.
- Trade-off: module boundaries remain the default. The native ASR runtime was isolated early after review showed that an in-process callback could not provide trustworthy wall-clock termination; other native model/codec boundaries remain evidence-driven.

## Implementation note — 2026-08-26

`whisper.cpp` inference now runs in one local child process behind bounded anonymous-pipe IPC. This is the narrow model-runtime exception anticipated by the decision, not a service or network architecture: the worker has no network protocol, persistence, UI, or model-path discovery capability, and the parent owns its lifecycle and hard deadline.

## Alternatives considered

- Local microservices: rejected because IPC and lifecycle complexity do not serve v1.
- Plugin architecture: rejected because arbitrary code loading violates the model supply-chain boundary.
