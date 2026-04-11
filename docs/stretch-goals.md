# Stretch Goals

Nice-to-have improvements that are not in the MVP or V1 roadmap. These are ideas worth capturing but not committing to a timeline.

## Data & Import
- OFX/QIF file import (in addition to CSV)
- Open Banking API integration for automatic transaction pulls
- Multi-currency support with automatic FX conversion
- Receipt photo scanning (OCR) for cash transactions

## Analysis & AI
- AI chat interface for querying finances ("how much did I spend on travel last quarter?")
- Anomaly detection (flag unusual spending patterns)
- Smart recurring transaction detection (auto-flag transactions that repeat monthly)

## Portfolio
- Real-time stock price fetching for portfolio valuation
- ETF composition drill-down (show underlying holdings)
- Tax-lot tracking for capital gains reporting

## Lifestyle Planning
- Tax planning for capital gains
- Early retirement planning (FIRE calculator)
- Rental income tracking
- Forecasting for big purchases (savings goal timeline)

## Integration
- Obsidian plugin for inline finance queries
- YNAB/Mint import for migration
- Export to common accounting formats

## UI/UX
- Mobile-responsive PWA
- Dark mode
- Customizable dashboard widgets

## Charting & Visualization
The frontend currently uses Tremor (built on Recharts) for all charts. For advanced chart types, drop to raw Recharts directly (same engine, no new dependency):
- Click-to-filter: clicking a pie slice to filter the transaction table view
- Synchronized cursors: hovering one chart highlights the same data point on another chart
- Custom animated transitions: chart morphing between view modes
- Candlestick / OHLC charts: if stock price visualization is added
- Waterfall charts: income-to-savings flow visualization
- Sankey diagrams: money flow between accounts
- Brush/zoom on charts: drag to select a time range on a chart to zoom in
