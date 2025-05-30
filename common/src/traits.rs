use anyhow::Result;
use serenity::{
    all::{
        CommandInteraction, Context, CreateCommand, CreateCommandOption, ModalInteraction,
        Permissions, ResolvedOption,
    },
    async_trait,
};
#[async_trait]
pub trait SubCommandTrait
where
    Self: Send + Sync,
{
    fn register_command(&self) -> CreateCommandOption;
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()>;
    fn command_name(&self) -> &str;
    fn permissions(&self) -> Permissions;
    #[allow(unused_variables)]
    async fn autocomplete(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        log::error!(
            "Autocomplete not implemented for {}",
            std::any::type_name::<Self>()
        );
        Ok(())
    }
}
#[async_trait]
pub trait CommandTrait
where
    Self: Send + Sync,
{
    fn register_command(&self) -> Option<CreateCommand> {
        None
    }
    fn aliases(&self) -> Vec<(&'static str, CreateCommand)> {
        vec![]
    }
    fn command_name(&self) -> &str {
        ""
    }
    #[allow(unused_variables)]
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        log::error!("Run not implemented for {}", self.command_name());
        Ok(())
    }
    fn modal_names(&self) -> &'static [&'static str] {
        &[]
    }
    #[allow(unused_variables)]
    async fn run_modal(&self, ctx: &Context, interaction: &ModalInteraction) -> Result<()> {
        log::error!(
            "Modal not implemented for {}",
            std::any::type_name::<Self>()
        );
        Ok(())
    }
    #[allow(unused_variables)]
    async fn autocomplete(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        log::error!(
            "Autocomplete not implemented for {}",
            std::any::type_name::<Self>()
        );
        Ok(())
    }
}
