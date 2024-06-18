//! The Underlying V4 cfmm curve. Transformed into a clob with N ticks
//! on each side.

use malachite::num::basic::traits::Zero;

/// Starts and ends at ticks but might not be a single tick if it spans
/// uninitialized ticks.
#[derive(Debug)]
pub struct PoolRange {
    // Liquidity range bounds.
    lower:     f64,
    upper:     f64,
    liquidity: f64,
    // (𝛥x, 𝛥y) The the net amounts required to fully jump the range.
    full_dx:   f64,
    full_dy:   f64
}

impl PoolRange {
    pub fn new(lower: f64, upper: f64, liquidity: f64) -> Self {
        // TODO: Use Result.
        assert!(upper > lower);
        assert!(lower > f64::ZERO);
        assert!(liquidity > f64::ZERO);
        let full_dx = (1.0 / lower.sqrt() - 1.0 / upper.sqrt()) * liquidity;
        let full_dy = (lower.sqrt() - upper.sqrt()) * liquidity;
        Self { lower, upper, liquidity, full_dx, full_dy }
    }

    fn partial_dx(&self, from_price: f64, to_price: f64) -> f64 {
        (1.0 / to_price.sqrt() - 1.0 / from_price.sqrt()) * self.liquidity
    }

    fn partial_dy(&self, from_price: f64, to_price: f64) -> f64 {
        (to_price.sqrt() - from_price.sqrt()) * self.liquidity
    }
}

/// UniswapV4 Ticks in a book format.
/// Math: https://www.desmos.com/calculator/kh8ckngzap
#[derive(Debug)]
pub struct UniswapV4Book<const BOUND: usize> {
    price:       f64,
    range_index: usize,
    ranges:      [PoolRange; BOUND]
}

impl<const BOUND: usize> UniswapV4Book<BOUND> {
    pub fn new(price: f64, range_index: usize, ranges: [PoolRange; BOUND]) -> Self {
        assert!(range_index < BOUND);
        assert!(ranges.windows(2).all(|w| w[0].upper == w[1].lower));
        assert!(ranges[range_index].lower <= price && price <= ranges[range_index].upper);
        Self { price, range_index, ranges }
    }

    pub fn get_price(&self) -> f64 {
        self.price
    }

    fn current(&self) -> Option<&PoolRange> {
        self.ranges.get(self.range_index)
    }

    pub fn swap_to_limit(&mut self, limit: f64) -> Option<(f64, f64)> {
        let mut dx: f64 = f64::ZERO;
        let mut dy: f64 = f64::ZERO;

        let mut current = &self.ranges[self.range_index];
        let mut last_price = self.price;

        if limit < current.lower {
            // Swap to end of current.
            dx += current.partial_dx(last_price, current.lower);
            dy += current.partial_dy(last_price, current.lower);

            loop {
                self.range_index = self.range_index.checked_sub(1)?;
                current = self.current()?;
                if current.lower < limit {
                    break;
                }
                dx += current.full_dx;
                dy += current.full_dy;
                last_price = current.lower;
            }
        } else if current.upper < limit {
            // Swap to end of current.
            dx += current.partial_dx(last_price, current.upper);
            dy += current.partial_dy(last_price, current.upper);

            loop {
                self.range_index = self.range_index.checked_add(1)?;
                current = self.current()?;
                if limit < current.upper {
                    break;
                }
                dx -= current.full_dx;
                dy -= current.full_dy;
                last_price = current.upper;
            }
        }

        dx += current.partial_dx(last_price, limit);
        dy += current.partial_dy(last_price, limit);

        self.price = limit;

        Some((dx, dy))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_basic_range() {
        println!("\ntest_basic_range");
        let range = PoolRange::new(3000.0, 3100.0, 3400.0);
        println!("range.full_dx: {}", range.full_dx);
        println!("range.full_dy: {}", range.full_dy);

        let dx = range.partial_dx(range.lower, 3050.0);
        let dy = range.partial_dy(range.lower, 3050.0);

        println!("dx: {}", dx);
        println!("dy: {}", dy);

        let ranges = [
            PoolRange::new(2000.0, 2100.0, 1000.0),
            PoolRange::new(2100.0, 2200.0, 1000.0),
            PoolRange::new(2200.0, 2300.0, 1000.0),
            PoolRange::new(2300.0, 2400.0, 1000.0),
            PoolRange::new(2400.0, 2500.0, 1000.0)
        ];

        let mut book = UniswapV4Book::new(2250.0, 2, ranges);

        let (dx, dy) = book.swap_to_limit(2200.0 - 1e-13).unwrap();
        println!("dx: {}", dx);
        println!("dy: {}", dy);
        println!("book.price: {}", book.price);
        println!("book.range_index: {}", book.range_index);
    }
}
