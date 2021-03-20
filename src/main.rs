use serenity::async_trait;
use serenity::client::{Client, Context, EventHandler};
use serenity::model::channel::{Message};
use serenity::model::prelude::{Channel, GuildChannel};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

use regex::Regex;

use rand::seq::SliceRandom;

const BOT_NAME: &str = "trevlig liten bot";

async fn handle_bot_command(ctx: &Context, message: &Message, content: &str) -> serenity::Result<()> {
	if content.contains("tack") {
		message.reply(&ctx, "Varsågod!!! :blush: :two_hearts:").await?;
	}
	else if content.contains("godnatt") {
		message.reply(&ctx, "Godnatt på dig :heart: :blush:").await?;
	}
	Ok(())
}

fn find_images_on_page<'a>(body: &'a str) -> Vec<&'a str> {
	let mut result = Vec::new();
	
	let extensions = Regex::new(".jpg|.png|.jpeg").unwrap();
	for i in extensions.find_iter(body) {
		let i = i.start();
		const QUOTE: char = '\"';
		if let Some(start) = body[..i].rfind(QUOTE) {
			if let Some(end) = body[i..].find(QUOTE) {
				result.push(&body[start+1..i+end]);
			}
		}
	}
	result
}

async fn respond_to_counting(ctx: &Context, channel: &GuildChannel, message: &str) -> serenity::Result<()> {
	let count = message.parse::<i32>()?;
	channel.send_message(ctx, |msg| msg.content(count + 1)).await?;
	Ok(())
}

async fn get_message_channel(ctx: &Context, message: &Message) -> Option<GuildChannel> {
	if let Some(Channel::Guild(channel)) = message.channel(&ctx).await {
		return Some(channel);
	}
	None
}

async fn remove_last_bot_message<F>(ctx: &Context, channel: GuildChannel, message_filter: F) -> bool 
	where F: Fn(&Message) -> bool
{
	if let Ok(messages) = channel.messages(&ctx, |b| b).await {
		if let Some(found) = messages.iter().find(|message| 
			message.author.name == BOT_NAME && message_filter(message)
		) {
			if found.delete(&ctx).await.is_ok() {
				return true;
			}
		}
	}
	false
}

struct NiceLittleBot {
	http_client: reqwest::Client,
}

impl NiceLittleBot {
	fn new() -> NiceLittleBot {
		NiceLittleBot {
			http_client: reqwest::Client::new(),
		}
	}

	async fn fetch_html(&self, url: &str) -> reqwest::Result<String> {
		Ok(self.http_client.get(url)
			.header("user-agent", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.90 Safari/537.36")
			.send().await?
			.text().await?)
	}

	async fn get_first_youtube_search_result(&self, query: &str) -> Option<String> {
		if let Ok(body) = self.fetch_html(&format!("https://www.youtube.com/results?search_query={}", 
			utf8_percent_encode(query, NON_ALPHANUMERIC))).await 
		{
			if let Some(pos) = body.find("/watch?v=") {
				if let Some(count) = body[pos..].find('"') {
					return Some(format!("https://www.youtube.com{}", &body[pos..pos+count]));
				}
			}
		}
		None
	}

	async fn get_random_google_image_result(&self, query: &str) -> Option<String> {
		if let Ok(body) = self.fetch_html(&format!("https://www.google.com/search?tbm=isch&q={}", 
			utf8_percent_encode(query, NON_ALPHANUMERIC))).await 
		{
			if let Some(start_pos) = body.find("key: 'ds:1'") {
				if let Some(end_pos) = body[start_pos..].find("</script>") {
					let urls = find_images_on_page(&body[start_pos..start_pos+end_pos]);
					if let Some(url) = urls.choose(&mut rand::thread_rng()) {
						return Some(String::from(*url));
					}
				}
			}
		}
		None
	}

	async fn handle_youtube_command(&self, ctx: &Context, message: &Message, query: &str) -> serenity::Result<()> {
		if let Some(result_url) = self.get_first_youtube_search_result(query).await {
			message.reply(ctx, &format!("Jag hittade den!!!\n{}", result_url)).await?;
		}
		else {
			message.reply(ctx, "Kunde inte hitta den :c").await?;
		}
		Ok(())
	}

	async fn handle_photo_command(&self, ctx: &Context, message: &Message, query: &str) -> serenity::Result<()> {
		if let Some(result_url) = self.get_random_google_image_result(query).await {
			message.reply(ctx, &format!("Ett foto just för dig :two_hearts:\n{}", result_url)).await?;
		}
		else {
			message.reply(ctx, "Kunde inte hitta någon sån :sob:").await?;
		}
		Ok(())
	}

	async fn respond_to_message(&self, ctx: &Context, message: &Message) -> serenity::Result<()> {
		let content = message.content.to_ascii_lowercase();

		const YOUTUBE_PREFIX: &str = "youtube ";
		const PHOTO_PREFIX: &str = "fotografera ";
		if content.starts_with(YOUTUBE_PREFIX) {
			self.handle_youtube_command(&ctx, &message, content.strip_prefix(YOUTUBE_PREFIX).unwrap()).await?;
		}
		else if content.starts_with(PHOTO_PREFIX) {
			self.handle_photo_command(&ctx, &message, content.strip_prefix(PHOTO_PREFIX).unwrap()).await?;
		}
		else if content.contains("dans") {
			message.reply(&ctx, "💃 *dansar* 💃").await?;
		}
		else if let Some(channel) = get_message_channel(&ctx, &message).await {
			if content == "ta bort videon, bot." {
				if !remove_last_bot_message(&ctx, channel, |m| m.content.contains("https://www.youtube.com/watch?v=")).await {
					message.reply(&ctx, "Jag kunde inte ta bort någon video :sob:").await?;
				}
			}
			else if content == "bot ta bort" {
				if !remove_last_bot_message(&ctx, channel, |_| true).await {
					message.reply(&ctx, "Jag kunde inte ta bort mitt senaste meddelande :sob: förlåt mig!!!").await?;
				}
			}
			else if channel.name == "räkna" {
				respond_to_counting(&ctx, &channel, &message.content).await?;
			}
			else if content.contains("bot") {
				handle_bot_command(&ctx, &message, &content).await?;
			}
		}
		Ok(())
	}
}

#[async_trait]
impl EventHandler for NiceLittleBot {
	async fn message(&self, ctx: Context, message: Message) {
		if message.author.name == BOT_NAME {
			return;
		}

		if let Err(error) = self.respond_to_message(&ctx, &message).await {
			eprintln!("Error responding to message: {}", error);
		}
	}
}

#[tokio::main]
async fn main() {
	const SECRET_VARIABLE_NAME: &str = "DISCORD_BOT_SECRET";
	let token = std::env::var(SECRET_VARIABLE_NAME)
		.expect(&format!("Couldn't find environment variable {}", SECRET_VARIABLE_NAME));

	let mut client = Client::builder(token)
		.event_handler(NiceLittleBot::new())
		.await.expect("Error creating client");

	println!("Loggar in den trevliga lilla botten!");

	// Start listening for events
	client.start().await.expect("Error starting client");
}
