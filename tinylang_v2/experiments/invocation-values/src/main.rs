mod heap_invocation;
#[cfg(any(feature = "a", feature = "b"))]
mod heap_wire;
mod invocation;
mod workload;

fn main() {
    if std::env::args().any(|arg| arg == "--heap") {
        heap_invocation::benchmark();
    } else {
        workload::benchmark();
    }
}
