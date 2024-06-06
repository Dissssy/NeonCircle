mod channel;
mod guild;
mod user;
// This crate is for LTS (Long Term Storage) of data for the Neon Circle Discord bot.
// Uses PostgreSQL as the database.
//
// user stores
//  the id for querying
//  a boolean to consent (or not) to process their microphone data.
//
// guild will store
//  the id for querying
//  the default song volume
//  the default radio volume
//  whether to read titles by default
//  the radio audio url
//  the radio data url
//  the empty channel timeout (a duration between 0 and 600 seconds)
//
// channel will be a map from a voice channel id to a text channel id, and usually be queried in reverse, getting a list of voice channels from a text channel id.
