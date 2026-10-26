use navree_macros::agent;

trait Agent {}

struct Assistant;

#[agent]
impl Agent for Assistant {
    #[tool]
    async fn undocumented(&self, run: &(), args: ()) -> Result<(), ()> {
        Ok(())
    }

    /// Not async.
    #[tool]
    fn blocking(&self, run: &()) -> Result<(), ()> {
        Ok(())
    }

    /// An unknown option.
    #[tool(dangerous)]
    async fn unknown_option(&self, run: &()) -> Result<(), ()> {
        Ok(())
    }

    /// `from` is for routes.
    #[tool]
    #[can(Read, Deal, from = "id")]
    async fn with_from(&self, run: &()) -> Result<(), ()> {
        Ok(())
    }

    /// Two arguments values.
    #[tool]
    async fn too_many(&self, run: &(), a: (), b: ()) -> Result<(), ()> {
        Ok(())
    }

    #[roles("admin")]
    fn not_a_tool(&self) {}
}

struct WithArguments;

#[agent(name = "x")]
impl Agent for WithArguments {}

struct Inherent;

#[agent]
impl Inherent {}

fn main() {}
