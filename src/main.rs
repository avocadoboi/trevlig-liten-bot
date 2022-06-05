use serenity::async_trait;
// use serenity::client::{Client, Context, EventHandler};
use serenity::model::channel::{Message};
use serenity::model::gateway::{Ready};
use serenity::model::prelude::*;
// use serenity::model::guild::Emoji;
use serenity::prelude::*;

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

use regex::Regex;

use rand::seq::SliceRandom;

use serde::{Serialize, Deserialize};

use chrono::prelude::*;

use std::sync::Mutex;
use std::collections::HashMap;

//----------------------------------------------

const BOT_NAME: &str = "trevlig liten bot";

const GAME_SAVE_FILE_NAME: &str = "data.json";

//----------------------------------------------

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

async fn fetch_html(http_client: &reqwest::Client, url: &str) -> reqwest::Result<String> {
	Ok(http_client.get(url)
		.header("user-agent", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/89.0.4389.90 Safari/537.36")
		.send().await?
		.text().await?)
}

//----------------------------------------------

async fn get_first_youtube_search_result(http_client: &reqwest::Client, query: &str) -> Option<String> {
	if let Ok(body) = fetch_html(http_client, &format!("https://www.youtube.com/results?search_query={}", 
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

async fn get_random_google_image_result(http_client: &reqwest::Client, query: &str) -> Option<String> {
	if let Ok(body) = fetch_html(http_client, &format!("https://www.google.com/search?tbm=isch&q={}", 
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

//----------------------------------------------

async fn get_message_channel(ctx: &Context, message: &Message) -> Option<GuildChannel> {
	if let Some(Channel::Guild(channel)) = message.channel(&ctx).await {
		return Some(channel);
	}
	None
}

//----------------------------------------------

async fn remove_last_bot_message<F>(ctx: &Context, channel: &GuildChannel, message_filter: F) -> bool 
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

//----------------------------------------------

// A player index and a point count, could be total points or points for a specific name
#[derive(Serialize, Deserialize, Debug)]
struct PlayerPoints {
	player_index: usize,
	points: i32
}

#[derive(Serialize, Deserialize, Debug)]
struct NameGameName {
	name: String,
	player_points: Vec<i32>,
	last_point_time: DateTime<Utc>
}

impl NameGameName {
	fn new(name: &str) -> NameGameName {
		NameGameName {
			name: String::from(name),
			player_points: Vec::new(),
			last_point_time: Utc.timestamp(0, 0)
		}
	}
}

#[derive(Serialize, Deserialize, Debug)]
struct NameGame {
	names: Vec<NameGameName>,
	players: Vec<String>,
	last_message_channel_id: u64,
	last_message_id: u64
}

struct NameScore {
	name_index: usize,
	player_index: usize
}

fn is_message_disqualified(message: &Message) -> bool {
	let lowercase_content = message.content.to_lowercase();
	
	let forbidden_pattern = Regex::new("poäng|point").unwrap();
	if forbidden_pattern.is_match(&lowercase_content) {
		return true;
	}

	if let Some(replied_message) = &message.referenced_message {
		if forbidden_pattern.is_match(&replied_message.content.to_lowercase()) {
			return true;
		}
	}
	false
}

impl NameGame {
	fn new() -> NameGame {
		NameGame {
			names: Vec::new(),
			players: Vec::new(),
			last_message_channel_id: 0,
			last_message_id: 0
		}
	}

	fn load() -> NameGame {
		serde_json::from_str(&std::fs::read_to_string("data.json").unwrap()).unwrap()
	}

	fn save(&self) {
		std::fs::write(GAME_SAVE_FILE_NAME, serde_json::to_string(&self).unwrap()).unwrap();
	}

	fn find_scored_name(&self, message: &Message, timestamp: DateTime<Utc>) -> Option<usize> {
		const MIN_MESSAGE_LENGTH: usize = 5;
		
		let name_matches: Vec<usize> = (0..self.names.len()).filter(|&i| 
				(message.content.contains(&self.names[i].name) || message.content.contains(&self.names[i].name.to_uppercase()))
				&& message.content.len() > self.names[i].name.len() + MIN_MESSAGE_LENGTH
			).collect();

		if name_matches.len() != 1 {
			return None;
		}

		let name_match = &self.names[name_matches[0]];

		if timestamp.date() == name_match.last_point_time.date() && 
			timestamp.hour() == name_match.last_point_time.hour() {
			return None;
		}

		Some(name_matches[0])
	}

	fn get_or_allocate_player_index(&mut self, player_name: &str) -> usize {
		if let Some(player_index) = self.players.iter().position(|p| p == &player_name) {
			return player_index;
		} else {
			self.players.push(String::from(player_name));
			for name in &mut self.names {
				name.player_points.push(0);
			}
			return self.players.len() - 1;
		}
	}

	// Returns the name that was scored and the index of the player that scored the name.
	fn check_message_for_point(&mut self, message: &Message, player_name: &str) -> Option<NameScore> {
		if is_message_disqualified(&message) {
			return None;
		}
		
		let timestamp = if let Some(timestamp) = message.edited_timestamp { timestamp } else { message.timestamp };

		if let Some(name_index) = self.find_scored_name(message, timestamp) {
			println!("Message \"{}\" by {} scored point!!", message.content, player_name);

			let player_index = self.get_or_allocate_player_index(player_name);

			let name = &mut self.names[name_index];
			name.player_points[player_index] += 1;
			name.last_point_time = timestamp;

			return Some(NameScore{name_index, player_index});
		}

		None
	}

	fn create_leaderboard_message(&self, emojis: &[Emoji]) -> String {
		let mut random_generator = rand::thread_rng();

		let mut reply = String::from("✨ TOPPLISTA ✨\n");
		for name in &self.names {
			let emoji = emojis.choose(&mut random_generator).expect("Server has no emojis!");
			reply += &format!("\n{} {} points:\n", emoji, name.name);
			
			let mut leaderboard_indices: Vec<_> = (0..name.player_points.len()).collect();
			leaderboard_indices.sort_by_key(|index| -name.player_points[*index]);

			for place in 0..leaderboard_indices.len() {
				let player_index = leaderboard_indices[place];
				let points = name.player_points[player_index];
				if points != 0 { // Assume zero points will be last place
					reply += &format!("{}. {}, {} poäng\n", place + 1, self.players[player_index], points);
				}
			}
		}

		reply += "\n<:skl:844921829428953138> **FLEST POÄNG SAMMANLAGT** <:bingus:825501356416434186>\n";

		let mut player_rank = Vec::with_capacity(self.players.len());
		for player_index in 0..self.players.len() {
			player_rank.push(PlayerPoints{
				player_index, 
				points: self.names.iter().map(|name| name.player_points[player_index]).sum()
			});
		}
		player_rank.sort_by_key(|player| -player.points);

		for place in 0..player_rank.len() {
			reply += &format!("{}. {}, {} poäng\n", 
				place + 1, 
				self.players[player_rank[place].player_index], 
				player_rank[place].points);
		}
		reply
	}
}

// fn compose_scoreboard_message(ctx: &Context) -> String {
// }

// fn does_message_get_name_point(content: &str) -> bool {

// }

//----------------------------------------------

struct NiceLittleBot {
	http_client: reqwest::Client,
	name_game: Mutex<NameGame>,	
}

impl NiceLittleBot {
	fn new() -> NiceLittleBot {
		NiceLittleBot {
			http_client: reqwest::Client::new(),
			name_game: Mutex::new(NameGame::load())
		}
	}

	async fn check_youtube_command(&self, ctx: &Context, message: &Message, lowercase_content: &str) -> serenity::Result<bool> {		
		const YOUTUBE_PREFIX: &str = "youtube ";
		if !lowercase_content.starts_with(YOUTUBE_PREFIX) {
			return Ok(false)
		}
		
		let query = lowercase_content.strip_prefix(YOUTUBE_PREFIX).unwrap();
		
		if let Some(result_url) = get_first_youtube_search_result(&self.http_client, query).await {
			message.reply(ctx, &format!("Jag hittade den!!!\n{}", result_url)).await?;
		}
		else {
			message.reply(ctx, "Kunde inte hitta den :c").await?;
		}
		Ok(true)
	}

	async fn check_photo_command(&self, ctx: &Context, message: &Message, lowercase_content: &str) -> serenity::Result<bool> {
		const PHOTO_PREFIX: &str = "fotografera ";
		if !lowercase_content.starts_with(PHOTO_PREFIX) {
			return Ok(false);
		}

		let query = lowercase_content.strip_prefix(PHOTO_PREFIX).unwrap();

		if let Some(result_url) = get_random_google_image_result(&self.http_client, query).await {
			message.reply(ctx, &format!("Ett foto just för dig :two_hearts:\n{}", result_url)).await?;
		}
		else {
			message.reply(ctx, "Kunde inte hitta någon sån :sob:").await?;
		}
		Ok(true)
	}

	async fn check_remove_command(&self, ctx: &Context, channel: &GuildChannel, message: &Message, lowercase_content: &str) 
		-> serenity::Result<bool> 
	{
		if lowercase_content == "ta bort videon, bot." {
			if !remove_last_bot_message(&ctx, &channel, |m| m.content.contains("https://www.youtube.com/watch?v=")).await {
				message.reply(&ctx, "Jag kunde inte ta bort någon video :sob:").await?;
			}
			return Ok(true);
		}
		else if lowercase_content == "bot ta bort" {
			if !remove_last_bot_message(&ctx, &channel, |_| true).await {
				message.reply(&ctx, "Jag kunde inte ta bort mitt senaste meddelande :sob: förlåt mig!!!").await?;
			}
			return Ok(true);
		}
		Ok(false)
	}

	async fn check_counting_message(&self, ctx: &Context, channel: &GuildChannel, content: &str) -> serenity::Result<bool> {
		if channel.name == "räkna" {
			if let Ok(count) = content.parse::<i32>() {
				channel.send_message(ctx, |msg| msg.content(count + 1)).await?;
				return Ok(true);
			}
		}
		Ok(false)
	}

	async fn check_points_message(&self, ctx: &Context, message: &Message, lowercase_content: &str) 
		-> serenity::Result<bool> 
	{
		if lowercase_content == "bot poäng" {
			let emojis = message.guild(ctx).await.unwrap().emojis(&ctx).await?;
			
			let reply = self.name_game.lock().unwrap().create_leaderboard_message(&emojis[..]);
			message.reply(&ctx, reply).await?;

			return Ok(true);
		}

		if let Some(player_name) = message.author_nick(ctx).await {
			let score = self.name_game.lock().unwrap().check_message_for_point(message, &player_name);

			if let Some(NameScore{name_index, player_index}) = score {
				let reply = {
					let name = &self.name_game.lock().unwrap().names[name_index];
					let new_points = name.player_points[player_index];

					format!("Grattis {}, du fick ett {}-poäng! Du har nu {} {}-poäng.", &player_name, name.name, new_points, name.name)
				};
				message.reply(ctx, &reply).await?;

				self.name_game.lock().unwrap().last_message_channel_id = message.channel_id.0;
				self.name_game.lock().unwrap().last_message_id = message.id.0;
				self.name_game.lock().unwrap().save();

				return Ok(true);
			}
		}

		Ok(false)
	}

	async fn check_bot_conversation_message(&self, ctx: &Context, message: &Message, lowercase_content: &str) -> serenity::Result<()> {
		if lowercase_content.contains("dans") {
			message.reply(&ctx, "💃 *dansar* 💃").await?;
		}
		
		if lowercase_content.contains("bot") {
			if lowercase_content.contains("tack") {
				message.reply(&ctx, "Varsågod!!! :blush: :two_hearts:").await?;
			}
			else if lowercase_content.contains("godnatt") {
				message.reply(&ctx, "Godnatt på dig :heart: :blush:").await?;
			}
		}
		Ok(())
	}

	async fn respond_to_message(&self, ctx: &Context, message: &Message) -> serenity::Result<()> {
		let channel = get_message_channel(&ctx, &message).await.unwrap();

		let content = message.content.to_ascii_lowercase();

		self.check_bot_conversation_message(&ctx, &message, &content).await?;

		let _ = self.check_youtube_command(&ctx, &message, &content).await? || 
			self.check_photo_command(&ctx, &message, &content).await? ||
			self.check_remove_command(&ctx, &channel, &message, &content).await? ||
			self.check_counting_message(&ctx, &channel, &message.content).await? ||
			self.check_points_message(&ctx, &message, &content).await?;

		Ok(())
	}
}

#[async_trait]
impl EventHandler for NiceLittleBot {
	async fn ready(&self, ctx: Context, _: Ready) {	
		println!("Ready counting points!\n");
		
		let channel = ChannelId(self.name_game.lock().unwrap().last_message_channel_id);
		let channel = match channel.to_channel(&ctx).await {
			Ok(Channel::Guild(channel)) => channel,
			_ => return
		};
		
		let mut nicknames: std::collections::HashMap<UserId, String> = std::collections::HashMap::new();

		let mut message_id = MessageId(self.name_game.lock().unwrap().last_message_id);

		loop {
			let messages = channel.messages(&ctx, |retriever| retriever.after(message_id).limit(100)).await.unwrap();

			if messages.is_empty() {
				break;
			}

			message_id = messages[0].id;

			println!("\nCounting points from {} more messages after {}...\n", messages.len(), messages[0].timestamp);

			for message in messages.iter().rev() {
				if message.author.name != BOT_NAME {
					if !nicknames.contains_key(&message.author.id) {
						if let Some(name) = message.author.nick_in(&ctx, channel.guild_id).await {
							nicknames.insert(message.author.id, name);
						}
						else {
							println!("Message author {} didn't have a nickname!", message.author.name);
							continue;
						}
					}

					let name = &nicknames[&message.author.id];
					self.name_game.lock().unwrap().check_message_for_point(&message, &name);
				}
			}
		}

		self.name_game.lock().unwrap().last_message_id = message_id.0;
		self.name_game.lock().unwrap().save();

		println!("Finished counting points!");
	}
	
	async fn message(&self, ctx: Context, message: Message) {
		if message.author.name == BOT_NAME {
			return;
		}

		if let Err(error) = self.respond_to_message(&ctx, &message).await {
			eprintln!("Error responding to message: {}", error);
		}
	}
}

fn init_data() {
	let game = NameGame {
		names: vec![
			NameGameName::new("Björn"),
			NameGameName::new("Noah"),
			NameGameName::new("Linnéa")
		],
		players: Vec::new(),
		last_message_channel_id: 486798341842141196,
		last_message_id: 912824919719563304
	};
	std::fs::write("data.json", serde_json::to_string(&game).unwrap()).unwrap();
}

async fn run_bot() {
	let mut client = Client::builder(env!("DISCORD_BOT_SECRET"))
		.event_handler(NiceLittleBot::new())
		.await.expect("Error creating client");

	println!("Loggar in den trevliga lilla botten!");

	// Start listening for events
	client.start().await.expect("Error starting client");
}

#[tokio::main]
async fn main() {
	// init_data();
	run_bot().await;
}
