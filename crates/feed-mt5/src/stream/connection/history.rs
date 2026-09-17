//! Page content and parked price context. Concurrent debt belongs to HistoryPager.
use crate::map::{PriceContext, TickMapper};
use quantick_engine::Trade;

pub(super) const MAX_TRADES_PER_PAGE: usize = 250_000;
struct PagedBlock {
    trades: Vec<Trade>,
    resume: PriceContext,
    over_cap: u64,
    opening: bool,
}

pub(super) struct CompletedPage {
    pub trades: Vec<Trade>,
    pub over_cap: u64,
    pub opening: bool,
}

#[derive(Default)]
pub(super) struct HistoryPage {
    page: Option<PagedBlock>,
}
impl HistoryPage {
    pub(super) fn collect(&mut self, trade: Trade) -> Option<Trade> {
        let Some(block) = self.page.as_mut() else {
            return Some(trade);
        };
        if block.trades.len() < MAX_TRADES_PER_PAGE {
            block.trades.push(trade);
        } else {
            block.over_cap = block.over_cap.saturating_add(1);
        }
        None
    }

    pub(super) fn start(&mut self, mapper: &mut TickMapper, opening: bool) {
        if let Some(stale) = self.page.take() {
            mapper.restore_price_context(stale.resume);
        }
        self.page = Some(PagedBlock {
            trades: Vec::new(),
            resume: mapper.take_price_context(),
            over_cap: 0,
            opening,
        });
    }

    pub(super) fn end(&mut self, mapper: &mut TickMapper) -> Option<CompletedPage> {
        let block = self.page.take()?;
        mapper.restore_price_context(block.resume);
        Some(CompletedPage {
            trades: block.trades,
            over_cap: block.over_cap,
            opening: block.opening,
        })
    }

    pub(super) fn len(&self) -> usize {
        self.page.as_ref().map_or(0, |page| page.trades.len())
    }
}
