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
