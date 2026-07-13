// EXEMPLAR: small atom-stack kernel driver -- scan -> hash -> project -> compare -> order,
//           with a typed error enum and inline unit tests. Dependency-free, std-only.
// tags: kernel, micro-kernel, medium-kernel, atom, atom-stack, scan, hash, project, compare,
//       order, pipeline, driver, scheduler, staged, enum, error-handling, exhaustive-match,
//       display, dependency-free, std-only, single-threaded
//
// Provenance: generated live by qwen/qwen3.5-122b-a10b for the operator intent
// "make a medium sized kernel", verified end-to-end -- rustc typecheck exit 0,
// `--test` build passes 6/6 inline tests, release binary runs and produces the
// expected atom-stack output. Seeded into the wiki-graph exemplar library so
// smaller models ADAPT this proven pattern instead of generating a kernel from
// weights that don't cover the shape.
//
// Anti-pattern this shape deliberately avoids: DO NOT reach for atomic types
// (AtomicBool, AtomicU64), locks (Mutex/RwLock), or worker-thread spawns for a
// "kernel" at this size. Small models (9b) will happily emit
// `#[derive(Clone)] struct Core { running: AtomicBool }` which fails E0277
// because atomics do not implement Clone; the model then burns every repair
// round trying to fix a self-inflicted invariant. This exemplar shows the plain
// single-threaded structure a kernel driver actually needs at this scale: a
// sequential atom-stack over &[u8] state.

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug)]
enum KernelError {
    ScanFailed,
    HashFailed,
    ProjectFailed,
    CompareFailed,
    OrderFailed,
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelError::ScanFailed => write!(f, "Scan stage failed"),
            KernelError::HashFailed => write!(f, "Hash stage failed"),
            KernelError::ProjectFailed => write!(f, "Project stage failed"),
            KernelError::CompareFailed => write!(f, "Compare stage failed"),
            KernelError::OrderFailed => write!(f, "Order stage failed"),
        }
    }
}

impl std::error::Error for KernelError {}

struct AtomKernel {
    input: Vec<u8>,
}

impl AtomKernel {
    fn new(input: &str) -> Self {
        Self {
            input: input.as_bytes().to_vec(),
        }
    }

    // Atom 1: Scan
    fn scan(&self) -> Result<Vec<u8>, KernelError> {
        if self.input.is_empty() {
            return Err(KernelError::ScanFailed);
        }
        Ok(self.input.clone())
    }

    // Atom 2: Hash
    fn hash(&self, data: &[u8]) -> Result<u64, KernelError> {
        let mut acc: u64 = 5381;
        for &byte in data {
            acc = acc.wrapping_mul(33).wrapping_add(byte as u64);
        }
        if acc == 0 {
            return Err(KernelError::HashFailed);
        }
        Ok(acc)
    }

    // Atom 3: Project
    fn project(&self, data: &[u8]) -> Result<Vec<u8>, KernelError> {
        let filtered: Vec<u8> = data
            .iter()
            .copied()
            .filter(|b| b.is_ascii_alphanumeric())
            .collect();
        if filtered.is_empty() {
            return Err(KernelError::ProjectFailed);
        }
        Ok(filtered)
    }

    // Atom 4: Compare
    fn compare(&self, left: u64, right: u64) -> Result<Ordering, KernelError> {
        Ok(left.cmp(&right))
    }

    // Atom 5: Order
    fn order(&self, data: &mut [u8]) -> Result<(), KernelError> {
        if data.is_empty() {
            return Err(KernelError::OrderFailed);
        }
        data.sort_unstable();
        Ok(())
    }

    fn execute(&self) -> Result<(), KernelError> {
        // Atom Stack Order: scan -> hash -> project -> compare -> order
        let scanned = self.scan()?;
        let hash_a = self.hash(&scanned)?;
        let projected = self.project(&scanned)?;
        let hash_b = self.hash(&projected)?;
        let order = self.compare(hash_a, hash_b)?;
        let mut mutable_projected = projected.clone();
        self.order(&mut mutable_projected)?;

        println!("Kernel Execution Complete");
        println!("  Hash A: {}", hash_a);
        println!("  Hash B: {}", hash_b);
        println!("  Comparison: {:?}", order);
        println!("  Projected Length: {}", mutable_projected.len());
        Ok(())
    }
}

fn main() {
    let kernel = AtomKernel::new("Atom Vibe Coder Kernel Build Plan");
    if let Err(e) = kernel.execute() {
        eprintln!("Error: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_valid() {
        let k = AtomKernel::new("valid");
        assert!(k.scan().is_ok());
    }

    #[test]
    fn test_scan_empty() {
        let k = AtomKernel::new("");
        assert!(k.scan().is_err());
    }

    #[test]
    fn test_hash_consistency() {
        let k = AtomKernel::new("");
        let data = b"test";
        let h1 = k.hash(data).unwrap();
        let h2 = k.hash(data).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_project_filter() {
        let k = AtomKernel::new("");
        let data = b"Hello World!";
        let proj = k.project(data).unwrap();
        assert!(!proj.contains(&b' '));
        assert!(!proj.contains(&b'!'));
    }

    #[test]
    fn test_order_sorts() {
        let k = AtomKernel::new("");
        let mut data = vec![3, 1, 2];
        k.order(&mut data).unwrap();
        assert_eq!(data, vec![1, 2, 3]);
    }

    #[test]
    fn test_full_pipeline() {
        let k = AtomKernel::new("Test Kernel");
        assert!(k.execute().is_ok());
    }
}
