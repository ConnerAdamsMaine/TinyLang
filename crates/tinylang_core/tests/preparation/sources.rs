//! Source fixtures shared by preparation allocation and phase diagnostics.

pub(super) struct SourceCase {
    pub(super) name:   &'static str,
    pub(super) source: &'static str,
    pub(super) output: &'static str,
}

pub(super) const SOURCE_CASES: [SourceCase; 4] = [
    SourceCase {
        name:   "print",
        source: "print 42",
        output: "42\n",
    },
    SourceCase {
        name:   "straightline",
        source: STRAIGHTLINE,
        output: "4\ntrue\n",
    },
    SourceCase {
        name:   "float_loop",
        source: FLOAT_LOOP,
        output: "2048.25\n",
    },
    SourceCase {
        name:   "functions",
        source: FUNCTIONS,
        output: "2736\n",
    },
];

const STRAIGHTLINE: &str = r"
let a = 1
let b = a + 2
let c = b * 3
let d = c - a
let e = d / 2
print e
print e >= 4
";

const FLOAT_LOOP: &str = r"
let i = 0
let total = 0.25
while i < 4096 {
  total = total + 0.5
  i = i + 1
}
print total
";

const FUNCTIONS: &str = r"
fn mul_by_count(value, count) {
  let acc = 0
  while count > 0 {
    acc = acc + value
    count = count - 1
  }
  return acc
}

fn pair(x) {
  return mul_by_count(x, 2) + mul_by_count(x + 1, 3)
}

let i = 1
let total = 0
while i <= 32 {
  total = total + pair(i)
  i = i + 1
}
print total
";
