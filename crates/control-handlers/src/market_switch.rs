//! The palette's switch mode, which the toolbar's market button opens. Enter
//! puts the tab on another market through [`set_source`], the one switch
//! `feed.source.set` and `tab.market.set` take, or rolls it through the act
//! `feed.contract.roll` runs. The trader's hand journals nothing, as the
//! window's other source changes do.
use crate::contract_roll::{self, ContractRollPort};
use crate::feed_source::set_source;
use crate::market_palette::MarketPaletteHost;
use crate::workspace_book::{WorkspaceBookHost, set_tab_workspace};
use quantick_control::wire::WireU64;
use quantick_engine::bar_registry::BUILTIN_BARS;
use quantick_stores::market_list::{
    RollOffer, SwitchChoice, SwitchTarget, roll_toast, switch_toast,
};

/// What a switch host can do: the window's switch, its roll, its palette and
/// its workspace book.
pub trait SwitchHost: ContractRollPort + MarketPaletteHost + WorkspaceBookHost {}
impl<H: ContractRollPort + MarketPaletteHost + WorkspaceBookHost + ?Sized> SwitchHost for H {}

const TAB_GONE: &str = "The tab being switched has closed";

/// The active tab as the switch palette opens on it, with the roll its feed
/// config names.
///
/// # Errors
/// The active tab is a recording, which has no market to switch.
pub fn target<H: SwitchHost + ?Sized>(app: &H) -> Result<SwitchTarget, String> {
    let roll = contract_roll::offer(app).map(|(target, refusal)| RollOffer {
        target,
        refusal: refusal.map(str::to_owned),
    });
    let tab = app.tab_id_at(app.active_tab_index());
    SwitchTarget::of(&app.market_snapshot(), tab, roll)
        .ok_or_else(|| "A recorded session has no market to switch".to_owned())
}

/// Put the tab on `choice`'s market, then on the bar and workspace the
/// trader picked, if any. Everything that could refuse is asked before the
/// tab or the catalog changes. Answers the toast.
///
/// # Errors
/// The refusal the palette shows under its field.
pub fn switch<H: SwitchHost + ?Sized>(
    app: &mut H,
    choice: &SwitchChoice,
) -> Result<String, String> {
    let bar = (choice.bar.as_deref())
        .map(|bar| (BUILTIN_BARS.parse(bar)).map_err(|error| format!("Bars {bar}: {error}")))
        .transpose()?;
    if let Some(name) = &choice.workspace {
        app.market_snapshot().check_workspace(name)?;
    }
    if choice.add {
        app.edit_catalog(&choice.feed_id, &choice.symbol, true)?;
    }
    let tab = WireU64::new(choice.tab_id);
    set_source(app, tab, &choice.feed_id, &choice.symbol).map_err(|error| error.message)?;
    if let Some(name) = &choice.workspace {
        set_tab_workspace(app, choice.tab_id, name).map_err(|error| error.message)?;
    }
    if let Some(bar) = bar {
        app.set_flow_bar(choice.tab_id, bar)?;
    }
    Ok(switch_toast(&choice.symbol))
}

/// Roll tab `tab_id` from `from_symbol` to the contract its feed names next,
/// asking the roll's own rule and running its own act.
///
/// # Errors
/// The roll's refusal, or the tab moved on since the palette opened.
pub fn roll<H: SwitchHost + ?Sized>(
    app: &mut H,
    tab_id: u64,
    from_symbol: &str,
) -> Result<String, String> {
    let index =
        crate::tabs::tab_index(app, Some(WireU64::new(tab_id))).map_err(|error| error.message)?;
    let current = app.feed_selection(index).ok_or(TAB_GONE)?;
    if current.symbol != from_symbol {
        return Err(format!("The tab no longer shows {from_symbol}"));
    }
    let next = (contract_roll::target(&*app, index))
        .ok_or(TAB_GONE)?
        .map_err(|refusal| refusal.reason().to_owned())?;
    contract_roll::apply(app, index, &next).ok_or(TAB_GONE)?;
    Ok(roll_toast(&next))
}
