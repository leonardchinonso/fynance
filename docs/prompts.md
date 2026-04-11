# Prompt 1: Initialization [DONE]

I want to build a project to track my finances. I should be able to provide statements for the last two-three years as a starter and it should organize it into sections or categories. Based on that data, we can then start with how to budget future income. Research all that is needed for this project to happen and write your results to `./research/` folder. Then come up with a plan to execute it in detail from beginning to end. Include design approaches, diagrams, code examples, pros and cons in great detail and write it to `./plans/`. The project should be written in Rust. Use this information to populate the CLAUDE.md file for this project too

# Prompt 1.1: Initialization Iteration

The requirements for this project have changed a little, now I want to do the following:

1. ⁠Ingest my spending and assign them to categories (banks already do this so i can literally just ingest a csv exported straight from monzo/revolut/lloyds etc)
2. A bugdet tab that shows me overall spending per month per category (or per whatever other filter i can come up with later on)
3. ⁠Portfolio overview: How much i have in different accounts. Diversity overview to be able to see how much in different secotrs different forms of money e.t.c
4. A good UI to show all the above requirements. It should optimize for good user experience and good visuals.
5. Security is paramount! Many users will use this solution, so we want to keep the Databases and storage layer completely local and isolated for each user. Each user shoule be able to start up the whole service by running a command and would be able to interact with the UI for queries and views.
6. The backend should be written in Rust.
7. Come up with design documents in `./design/` and plans in `./plans/` that compare and contrast different approaches with their various pros and cons for my review. Optimize for a design MVP that does not make many performance/usability sacrifices.

# Prompt 1.2: Detailed Requirements & Frontend Specification

## High-Level Objectives

This is primarily a **budgeting tracker** and **portfolio tracker** with a strong emphasis on visualization and interactivity.

## 1. Budget Tracking

### Core: Ingest and Track Spending

- Bulk ingest spending data, typically on a **monthly cadence** (end-of-month finances review)
- Ingestion methods:
  - CSV upload (from Monzo, Revolut, Lloyds, etc.) where categories may already be assigned by the bank
  - Screenshot upload: send images of banking apps to Claude, which extracts transaction data and hits an API to bulk ingest
  - Programmatic API: agents or scripts can generate a token and hit the API directly with CSV files or large data payloads
- Transactions should be categorized (by bank-provided categories, rule-based matching, or Claude AI)

### Guided Monthly Ingestion Flow

- On initial setup, configure all the places where you have money (Monzo, Revolut, Trading 212, pension, etc.)
- The app then provides a **guided flow** each month: "Now give me your Monzo", "Now give me your Revolut", "Now give me your Trading 212"
- Acts as a checklist so you do not forget to input data from any account
- Flow tracks which accounts have been updated this month and which are still pending

### Budget Visualization

- **Table view**: spending per category per month, similar to a spreadsheet
  - Color coding: **red** for categories where spending exceeds budget, **green** for categories under budget
- **Graph view**: bar charts, line charts showing spending over time per category
- **Pie chart view**: interactive pie chart where mousing over a section shows a tooltip (e.g., "Feeding: 23%, £3,500")
- All charts should be interactive: hover for details, click to drill down
- Ability to **export different views** (images, CSV, markdown)

### Time Navigation (Universal Across All Views)

- A **date range selector** always visible at the top of the screen
- Default shortcut views:
  - Current month
  - Last 3 months
  - Year-to-date
  - Full year (aggregated monthly)
  - Last 5 years (aggregated monthly or yearly)
  - Custom range
- Ability to **zoom in and out**: view Q1 2024, or just March 2024, or all of 2023
- **Sliding window**: pan forward and backward through time
- All visualizations update reactively when the date range changes

### Budget Planning

- View historical spending per category across many months to inform future budget decisions (e.g., "How much should I allocate to feeding? Let me see what I have spent over the last 12 months")
- Adjust budget amounts per category based on trends

## 2. Portfolio Tracking

### Core: Account Balance Visibility

- See account balances across all places where money is held: Monzo, Revolut, Trading 212, pension, savings, etc.
- Same visualization tools as budgeting: bar charts (total value over time), pie charts (allocation), tables
- Ability to **filter and toggle**: check/uncheck sources to include or exclude from the visualization (e.g., "remove everything in housing", "remove pension", "show only liquid assets")

### Stock-Level Detail

- For investment accounts (e.g., Trading 212), track individual holdings: how much in Stock A vs Stock B
- For ETFs, show the breakdown of what the ETF contains
- Ability to drill into a specific stock or ETF to see its composition and performance
- This data comes from ingesting account exports from these platforms

### Point-in-Time Data Model

- If I record £5,000 in a stock as of January 2023, and I do not update until April 2023, the app should show the January value as the latest known balance for February and March
- "Carry forward" the last known value until new data is provided
- When zooming into a month where no new data was entered, display the most recent prior value with an indicator that it is stale (e.g., "as of Jan 2023")

### Portfolio Visualization

- Net worth over time (line chart)
- Allocation by account type (pie/donut chart)
- Allocation by institution (pie/donut chart)
- Individual account balance trends (filterable)
- Cash flow: income vs spending per month (bar chart)

## 3. Universal Interaction Requirements

These apply to every view in the app (budget, portfolio, reports):

- **Interactive charts**: hover tooltips, click to drill down, responsive to date range changes
- **Multiple view modes**: table, bar chart, line chart, pie chart, all showing the same underlying data
- **Filtering**: by category, by account, by institution, by account type, with multi-select toggles
- **Date range selector**: always visible, with presets and custom range, zoom/pan
- **Export**: any view should be exportable as image, CSV, or markdown
- **Color coding**: red for over-budget, green for under-budget (budget view); consistent color palette across charts

## 4. V1+ Future Plans (Post-MVP)

1. **Forecasting**: based on historical spending patterns, project future spending, income, and net worth. "If I continue spending at this rate and earning at this rate, what will my net worth look like in 6/12/24 months?"
2. **Big purchase planning**: set a savings target and date, track progress, project when you will hit it based on current savings rate
3. **Early retirement modeling**: starting balance, spending rate, investment returns, project account balance over time
4. **Tax planning**: capital gains tracking, allowance usage
5. **Rental income tracking**: income and expense tracking for rental properties, useful for self-assessment
6. **AI chat interface**: conversational window to ask questions about your finances or dump screenshots for extraction

## 5. Today's Objective

1. Iterate on the **data model** to ensure it supports all of the above (especially point-in-time carry-forward for portfolio, stock-level holdings, and the guided ingestion flow)
2. Build out the **React frontend** with mock data: get the app to a state where you can click around, navigate between views, interact with charts, and validate the UX before wiring up real data

# Prompt 2: Implementation

Looking at the `/Users/leonard/projects/fynance/docs` folder, come up with an implementation plan for how we would build the backend MVP. The plan should be split into phases, with each phase outlining in detail what needs to be done to mark the phase as complete. Each phase should be self-contained and aim to build one part of the final service. Follow the outlined plans in `/Users/leonard/projects/fynance/docs/plans` while adhering to the designs in `/Users/leonard/projects/fynance/docs/design`. DO NOT WRITE ANY CODE YET. Write the implementation plan to the `/Users/leonard/projects/fynance/docs/plans/` folder and let me review it. The aim is to use this plan as a checklist and checkpoint for what is done and what is left to do, while referencing the plans and designs as a guide/blueprint.

# Prompt 3.1: Backend Phase 1 Implementation

Using the implementation plan in `/Users/leonard/projects/fynance/docs/plans/09_backend_implementation_plan.md`, implement phase 1. Add comments to explain complex code cases. If you're in doubt about whether the code is straightforward to understand to the average person, write a comment for it. When you're done implementing phase 1, create a new branch for the feature, commit the code and push it to that new branch. DO NOT move on to phase 2 until I tell you to do so. Implement phase 1 only and make changes to the backend only, do not make any frontend changes.

# Prompt 3.2: Backend Phase 2 Implementation
Using the implementation plan in `/Users/leonard/projects/fynance/docs/plans/09_backend_implementation_plan.md`, implement phase 2. Add comments to explain complex code cases. If you're in doubt about whether the code is straightforward to understand to the average person, write a comment for it. When you're done implementing phase 2, create a new branch for the feature, commit the code and push it to that new branch. DO NOT move on to phase 3 until I tell you to do so. Implement phase 2 only and make changes to the backend only, do not make any frontend changes.