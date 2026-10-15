//! Random programs for `tools/check_grammar.py`: `sample SEED COUNT` prints
//! COUNT of them, programs and fragments in turn, each after a line
//! `-- sample N program --` or `-- sample N fragment --`.

use embryo_systems::adt81::sample;

fn main() {
    let args: Vec<u64> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("usage: sample SEED COUNT"))
        .collect();
    let [seed, count] = args[..] else {
        panic!("usage: sample SEED COUNT");
    };
    for n in seed..seed + count {
        if n % 2 == 0 {
            print!("-- sample {n} program --\n{}", sample::program(n));
        } else {
            print!("-- sample {n} fragment --\n{}", sample::fragment(n));
        }
    }
}
