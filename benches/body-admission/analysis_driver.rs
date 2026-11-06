// Compiled by analysis_benchmark.py with source-extracted baseline/candidate modules.
mod baseline;
mod candidate;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

struct CountingAllocator;
static COUNT: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT.load(Relaxed) {
            CALLS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size(), Relaxed);
        }
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNT.load(Relaxed) {
            CALLS.fetch_add(1, Relaxed);
            BYTES.fetch_add(size, Relaxed);
        }
        System.realloc(ptr, layout, size)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let iterations: usize = args[1].parse().unwrap();
    let repetitions: usize = args[2].parse().unwrap();
    let mut route =
        gateway_core::Route::resolve_match(&http::Method::POST, "/v1/chat/completions").unwrap();
    route.max_body_bytes = 100 * 1024 * 1024;
    for size in [1024, 5 * 1024 * 1024, 10 * 1024 * 1024] {
        for shape in ["flat", "messages"] {
            let value = if shape == "flat" {
                serde_json::json!({"model":"gpt-test", "input":"x".repeat(size), "max_tokens":12})
            } else {
                let messages: Vec<_> = (0..128)
                    .map(|_| serde_json::json!({"role":"user", "content":"x".repeat(size / 128)}))
                    .collect();
                serde_json::json!({"model":"gpt-test", "messages":messages, "max_tokens":12})
            };
            let body = bytes::Bytes::from(serde_json::to_vec(&value).unwrap());
            drop(value);
            for (name, input, tpm) in [
                ("none", false, false),
                ("input", true, false),
                ("tpm", false, true),
                ("both", true, true),
            ] {
                let policy = gateway_core::KeyPolicy {
                    max_input_tokens_per_request: input.then_some(i32::MAX),
                    tpm_limit: tpm.then_some(i32::MAX),
                    ..Default::default()
                };
                let functions: [fn(
                    bytes::Bytes,
                    &gateway_core::KeyPolicy,
                    &gateway_core::RouteMatch,
                ) -> i64; 2] = [baseline::run, candidate::run];
                assert_eq!(
                    functions[0](body.clone(), &policy, &route),
                    functions[1](body.clone(), &policy, &route)
                );
                for repetition in 0..repetitions {
                    for arm in if repetition % 2 == 0 { [0, 1] } else { [1, 0] } {
                        let function = functions[arm];
                        // Time without allocator instrumentation, then count a
                        // separate invocation so atomic updates do not bias CPU.
                        let start = Instant::now();
                        for _ in 0..iterations {
                            std::hint::black_box(function(
                                std::hint::black_box(body.clone()),
                                &policy,
                                &route,
                            ));
                        }
                        let ns_per_body = start.elapsed().as_nanos() as f64 / iterations as f64;
                        CALLS.store(0, Relaxed);
                        BYTES.store(0, Relaxed);
                        COUNT.store(true, Relaxed);
                        std::hint::black_box(function(body.clone(), &policy, &route));
                        COUNT.store(false, Relaxed);
                        let arm_name = ["baseline", "candidate"][arm];
                        println!(
                            "{}",
                            serde_json::json!({"arm":arm_name, "payload_bytes":size, "body_bytes":body.len(), "shape":shape, "policy":name, "repetition":repetition+1, "iterations":iterations, "ns_per_body":ns_per_body, "allocation_calls":CALLS.load(Relaxed), "allocated_bytes":BYTES.load(Relaxed)})
                        );
                    }
                }
            }
        }
    }
}
