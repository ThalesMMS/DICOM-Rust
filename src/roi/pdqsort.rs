fn less(x: f64, y: f64) -> bool {
    (x.is_nan() && !y.is_nan()) || x < y
}
pub(crate) fn sort_float64s(x: &mut [f64]) {
    let n = x.len();
    let limit = (usize::BITS - n.leading_zeros()) as usize;
    pdqsort(x, 0, n, limit);
}
#[allow(dead_code)]
pub(crate) fn sort_ints(x: &mut [i64]) {
    x.sort_unstable();
}
fn insertion_sort(data: &mut [f64], a: usize, b: usize) {
    for i in a + 1..b {
        let mut j = i;
        while j > a && less(data[j], data[j - 1]) {
            data.swap(j, j - 1);
            j -= 1;
        }
    }
}
fn sift_down(data: &mut [f64], lo: usize, hi: usize, first: usize) {
    let mut root = lo;
    loop {
        let mut child = 2 * root + 1;
        if child >= hi {
            break;
        }
        if child + 1 < hi && less(data[first + child], data[first + child + 1]) {
            child += 1;
        }
        if !less(data[first + root], data[first + child]) {
            return;
        }
        data.swap(first + root, first + child);
        root = child;
    }
}
fn heap_sort(data: &mut [f64], a: usize, b: usize) {
    let first = a;
    let lo = 0;
    let hi = b - a;
    let mut i = (hi as isize - 1) / 2;
    while i >= 0 {
        sift_down(data, i as usize, hi, first);
        i -= 1;
    }
    let mut i = hi as isize - 1;
    while i >= 0 {
        data.swap(first, first + i as usize);
        sift_down(data, lo, i as usize, first);
        i -= 1;
    }
}
#[derive(PartialEq, Clone, Copy)]
enum Hint {
    Unknown,
    Increasing,
    Decreasing,
}
fn pdqsort(data: &mut [f64], mut a: usize, mut b: usize, mut limit: usize) {
    const MAX_INSERTION: usize = 12;
    let mut was_balanced = true;
    let mut was_partitioned = true;
    loop {
        let length = b - a;
        if length <= MAX_INSERTION {
            insertion_sort(data, a, b);
            return;
        }
        if limit == 0 {
            heap_sort(data, a, b);
            return;
        }
        if !was_balanced {
            break_patterns(data, a, b);
            limit -= 1;
        }
        let (mut pivot, mut hint) = choose_pivot(data, a, b);
        if hint == Hint::Decreasing {
            reverse_range(data, a, b);
            pivot = (b - 1) - (pivot - a);
            hint = Hint::Increasing;
        }
        if was_balanced
            && was_partitioned
            && hint == Hint::Increasing
            && partial_insertion_sort(data, a, b)
        {
            return;
        }
        if a > 0 && !less(data[a - 1], data[pivot]) {
            a = partition_equal(data, a, b, pivot);
            continue;
        }
        let (mid, already_partitioned) = partition(data, a, b, pivot);
        was_partitioned = already_partitioned;
        let (left_len, right_len) = (mid - a, b - mid);
        let balance_threshold = length / 8;
        if left_len < right_len {
            was_balanced = left_len >= balance_threshold;
            pdqsort(data, a, mid, limit);
            a = mid + 1;
        } else {
            was_balanced = right_len >= balance_threshold;
            pdqsort(data, mid + 1, b, limit);
            b = mid;
        }
    }
}
fn partition(data: &mut [f64], a: usize, b: usize, pivot: usize) -> (usize, bool) {
    data.swap(a, pivot);
    let (mut i, mut j) = (a + 1, b - 1);
    while i <= j && less(data[i], data[a]) {
        i += 1;
    }
    while i <= j && !less(data[j], data[a]) {
        j -= 1;
    }
    if i > j {
        data.swap(j, a);
        return (j, true);
    }
    data.swap(i, j);
    i += 1;
    j -= 1;
    loop {
        while i <= j && less(data[i], data[a]) {
            i += 1;
        }
        while i <= j && !less(data[j], data[a]) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i, j);
        i += 1;
        j -= 1;
    }
    data.swap(j, a);
    (j, false)
}
fn partition_equal(data: &mut [f64], a: usize, b: usize, pivot: usize) -> usize {
    data.swap(a, pivot);
    let (mut i, mut j) = (a + 1, b - 1);
    loop {
        while i <= j && !less(data[a], data[i]) {
            i += 1;
        }
        while i <= j && less(data[a], data[j]) {
            j -= 1;
        }
        if i > j {
            break;
        }
        data.swap(i, j);
        i += 1;
        j -= 1;
    }
    i
}
fn partial_insertion_sort(data: &mut [f64], a: usize, b: usize) -> bool {
    const MAX_STEPS: usize = 5;
    const SHORTEST_SHIFTING: usize = 50;
    let mut i = a + 1;
    for _ in 0..MAX_STEPS {
        while i < b && !less(data[i], data[i - 1]) {
            i += 1;
        }
        if i == b {
            return true;
        }
        if b - a < SHORTEST_SHIFTING {
            return false;
        }
        data.swap(i, i - 1);
        if i - a >= 2 {
            let mut j = i - 1;
            while j >= 1 {
                if !less(data[j], data[j - 1]) {
                    break;
                }
                data.swap(j, j - 1);
                j -= 1;
            }
        }
        if b - i >= 2 {
            let mut j = i + 1;
            while j < b {
                if !less(data[j], data[j - 1]) {
                    break;
                }
                data.swap(j, j - 1);
                j += 1;
            }
        }
    }
    false
}
fn break_patterns(data: &mut [f64], a: usize, b: usize) {
    let length = b - a;
    if length >= 8 {
        let mut random = length as u64;
        let modulus = 1u64 << (usize::BITS - length.leading_zeros());
        let idx0 = a + (length / 4) * 2 - 1;
        for idx in idx0..=idx0 + 2 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let mut other = (random & (modulus - 1)) as usize;
            if other >= length {
                other -= length;
            }
            data.swap(idx, a + other);
        }
    }
}
fn choose_pivot(data: &[f64], a: usize, b: usize) -> (usize, Hint) {
    const SHORTEST_NINTHER: usize = 50;
    const MAX_SWAPS: usize = 4 * 3;
    let l = b - a;
    let mut swaps = 0usize;
    let mut i = a + l / 4;
    let mut j = a + l / 4 * 2;
    let mut k = a + l / 4 * 3;
    if l >= 8 {
        if l >= SHORTEST_NINTHER {
            i = median(data, i - 1, i, i + 1, &mut swaps);
            j = median(data, j - 1, j, j + 1, &mut swaps);
            k = median(data, k - 1, k, k + 1, &mut swaps);
        }
        j = median(data, i, j, k, &mut swaps);
    }
    match swaps {
        0 => (j, Hint::Increasing),
        MAX_SWAPS => (j, Hint::Decreasing),
        _ => (j, Hint::Unknown),
    }
}
fn order2(data: &[f64], a: usize, b: usize, swaps: &mut usize) -> (usize, usize) {
    if less(data[b], data[a]) {
        *swaps += 1;
        return (b, a);
    }
    (a, b)
}
fn median(data: &[f64], a: usize, b: usize, c: usize, swaps: &mut usize) -> usize {
    let (a, b) = order2(data, a, b, swaps);
    let (b, _c) = order2(data, b, c, swaps);
    let (_a, b) = order2(data, a, b, swaps);
    b
}
fn reverse_range(data: &mut [f64], a: usize, b: usize) {
    data[a..b].reverse();
}
