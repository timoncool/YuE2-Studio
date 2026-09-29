use std::{env, fs, path::{Path, PathBuf}, sync::{Arc, Mutex}};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Library { connection: Arc<Mutex<Connection>>, media_dir: PathBuf }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Song { pub id:String, pub title:String, pub audio_path:Option<String>, pub caption:String, pub lyrics:String, pub metadata:serde_json::Value, pub generation_settings:serde_json::Value, pub engine_id:String, pub profile_id:Option<String>, pub replay_request:Option<serde_json::Value>, pub audio_codes:Option<serde_json::Value>, pub source:String, pub created_at:String, pub updated_at:String }
#[derive(Debug, Clone, Deserialize)]
pub struct SongInput { pub title:String, pub audio_path:Option<String>, #[serde(default)] pub caption:String, #[serde(default)] pub lyrics:String, #[serde(default)] pub metadata:serde_json::Value, #[serde(default)] pub generation_settings:serde_json::Value, pub engine_id:String, pub profile_id:Option<String>, pub replay_request:Option<serde_json::Value>, pub audio_codes:Option<serde_json::Value>, #[serde(default="manual_source")] pub source:String }
#[derive(Debug, Clone)]
pub struct GeneratedSongInput { pub title:Option<String>, pub metadata:serde_json::Value, pub caption:String, pub lyrics:String, pub generation_settings:serde_json::Value, pub replay_request:Option<serde_json::Value>, pub audio_codes:Option<serde_json::Value>, pub engine_id:String, pub profile_id:Option<String>, pub source:String, pub audio_extension:&'static str, pub audio:Vec<u8> }
#[derive(Debug, Clone)]
pub struct AudioImportInput { pub title:String, pub caption:String, pub lyrics:String, pub metadata:serde_json::Value, pub generation_settings:serde_json::Value, pub engine_id:String, pub profile_id:Option<String>, pub source:String, pub audio_extension:String, pub audio:Vec<u8> }
#[derive(Debug, Clone, Serialize)]
pub struct ImportedSong { pub song: Song, pub audio_filename: String }
#[derive(Debug, Clone, Serialize, Deserialize)] pub struct Playlist { pub id:String, pub name:String, pub description:Option<String>, pub song_ids:Vec<String>, pub created_at:String, pub updated_at:String }
/// A stem that once was separated from a song and was put away when a new set took its place:
/// it keeps the link to the song it came from, so nothing a person made is ever orphaned.
#[derive(Debug, Clone, Serialize, Deserialize)] pub struct ArchivedStem { pub id:String, pub song_id:String, pub song_title:String, pub batch:String, pub batch_slug:String, pub stem:String, pub title:String, pub audio_path:String, pub duration_secs:Option<f64>, pub created_at:String, pub archived_at:String }
#[derive(Debug, Clone, Deserialize)] pub struct PlaylistInput { pub name:String, pub description:Option<String>, #[serde(default)] pub song_ids:Vec<String> }
fn manual_source()->String{"manual".into()}

/// Playable length measured from the audio itself.
///
/// The engine's replay request is sparse — a 60-second track omits the
/// `duration` field because 60 is the default — so the only dependable source
/// for a library row is the rendered file. WAV is read from its header; MP3 is
/// derived from the declared constant bitrate.
pub fn audio_duration_seconds(audio:&[u8],extension:&str,declared_bitrate_kbps:Option<u32>)->Option<f64>{
 match extension{
  "wav"=>{
   if audio.len()<44||&audio[0..4]!=b"RIFF"||&audio[8..12]!=b"WAVE"{return None}
   let fmt=audio.windows(4).position(|w|w==b"fmt ")?;
   let channels=u16::from_le_bytes(audio.get(fmt+10..fmt+12)?.try_into().ok()?) as f64;
   let rate=u32::from_le_bytes(audio.get(fmt+12..fmt+16)?.try_into().ok()?) as f64;
   let bits=u16::from_le_bytes(audio.get(fmt+22..fmt+24)?.try_into().ok()?) as f64;
   let data=audio.windows(4).position(|w|w==b"data")?;
   let payload=u32::from_le_bytes(audio.get(data+4..data+8)?.try_into().ok()?) as f64;
   let bytes_per_second=rate*channels*(bits/8.0);
   (bytes_per_second>0.0).then(||payload/bytes_per_second)
  }
  "mp3"=>{
   let bitrate=declared_bitrate_kbps.unwrap_or(128) as f64*1000.0;
   (bitrate>0.0).then(||(audio.len() as f64*8.0)/bitrate)
  }
  _=>None,
 }
}

/// A caption is a full style prompt, not a song name. Without an explicit
/// title the library shows a readable fragment instead of the whole prompt.
/// A caption written for this model is a labelled document, so its first line
/// is a section heading - "Global Metadata" - which makes a useless title. Skip
/// the headings and take the first line that actually describes the music.
const CAPTION_HEADINGS:[&str;5]=["global metadata","vocal details","arrangement","basic attributes","instrument lifecycle description"];
/// "bpm is 118" and "key is F# minor" describe the music but read terribly as a
/// name, so the title skips past them to the first descriptive phrase.
const CAPTION_ATTRIBUTES:[&str;5]=["bpm is","key is","scale is","tempo is","time signature"];
fn generated_title(caption:&str)->String{
 let first=caption.split(['\n','.',';']).map(str::trim)
  .find(|part|{
   let lowered=part.to_ascii_lowercase();
   !part.is_empty()
    && !CAPTION_HEADINGS.iter().any(|heading|lowered.starts_with(heading))
    && !CAPTION_ATTRIBUTES.iter().any(|attribute|lowered.contains(attribute))
  })
  .unwrap_or("Untitled track");
 let mut title=String::new();
 for word in first.split_whitespace(){
  if !title.is_empty() && title.chars().count()+1+word.chars().count()>48 {break}
  if !title.is_empty(){title.push(' ')}
  title.push_str(word);
 }
 if title.is_empty(){"Untitled track".into()}else{title}
}

/// What an image actually is, read off its first bytes.
///
/// Every picture format announces itself: a claim in a header or a filename is
/// a claim, this is the fact.
pub fn sniff_image_type(image:&[u8])->Option<&'static str>{
 if image.starts_with(&[0xFF,0xD8,0xFF]){return Some("image/jpeg")}
 if image.starts_with(&[0x89,b'P',b'N',b'G',0x0D,0x0A,0x1A,0x0A]){return Some("image/png")}
 if image.len()>=12&&&image[..4]==b"RIFF"&&&image[8..12]==b"WEBP"{return Some("image/webp")}
 None
}

#[cfg(test)]
mod image_type_tests {
    #[test]
    fn every_format_is_recognised_by_its_own_bytes() {
        assert_eq!(super::sniff_image_type(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(super::sniff_image_type(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]), Some("image/png"));
        let mut webp = b"RIFF0000WEBP".to_vec();
        webp.extend_from_slice(b"VP8 ");
        assert_eq!(super::sniff_image_type(&webp), Some("image/webp"));
        assert_eq!(super::sniff_image_type(b"not a picture"), None);
    }
}

impl Library {
 /// The library lives in the same place as models and settings. Resolving it
    /// separately used to put the database under `<cwd>/data` whenever the
    /// service was started outside the desktop shell, so the same install
    /// showed two different libraries depending on how it was launched.
    pub fn open_default()->Result<Self>{let root=crate::studio_data_root().unwrap_or_else(||env::current_dir().unwrap_or_else(|_|PathBuf::from(".")).join("data"));Self::open_at(root.join("library.sqlite"),root.join("media"))}
 pub fn open_at(db_path:PathBuf,media_dir:PathBuf)->Result<Self>{if let Some(p)=db_path.parent(){fs::create_dir_all(p)?};let c=Connection::open(db_path)?;c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS songs(id TEXT PRIMARY KEY,title TEXT NOT NULL,audio_path TEXT,caption TEXT NOT NULL,lyrics TEXT NOT NULL,metadata_json TEXT NOT NULL,generation_settings_json TEXT NOT NULL,engine_id TEXT NOT NULL,profile_id TEXT,replay_request_json TEXT,audio_codes_json TEXT,source TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS playlists(id TEXT PRIMARY KEY,name TEXT NOT NULL,description TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS playlist_songs(playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,song_id TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,position INTEGER NOT NULL,PRIMARY KEY(playlist_id,song_id)); CREATE TABLE IF NOT EXISTS workspaces(id TEXT PRIMARY KEY,name TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,opened_at TEXT,closed_at TEXT); CREATE TABLE IF NOT EXISTS workspace_songs(workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,song_id TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,position INTEGER NOT NULL,PRIMARY KEY(workspace_id,song_id)); CREATE TABLE IF NOT EXISTS playback_modes(context_key TEXT PRIMARY KEY,repeat_mode TEXT NOT NULL,shuffle INTEGER NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS session_agent(session_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,allow INTEGER NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS agent_settings(key TEXT PRIMARY KEY,value TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS session_kinds(session_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,kind TEXT NOT NULL,created_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS stems_archive(id TEXT PRIMARY KEY,song_id TEXT NOT NULL,song_title TEXT NOT NULL,batch TEXT NOT NULL,batch_slug TEXT NOT NULL,stem TEXT NOT NULL,title TEXT NOT NULL,audio_path TEXT NOT NULL,duration_secs REAL,created_at TEXT NOT NULL,archived_at TEXT NOT NULL);")?;let library=Self{connection:Arc::new(Mutex::new(c)),media_dir};library.read_sets_of_stems_without_words()?;Ok(library)}
 pub fn list_songs(&self)->Result<Vec<Song>>{let c=self.connection.lock().unwrap();let mut s=c.prepare("SELECT id,title,audio_path,caption,lyrics,metadata_json,generation_settings_json,engine_id,profile_id,replay_request_json,audio_codes_json,source,created_at,updated_at FROM songs ORDER BY created_at DESC")?;Ok(s.query_map([],row_song)?.collect::<rusqlite::Result<_>>()?)}
 pub fn get_song(&self,id:&str)->Result<Option<Song>>{let c=self.connection.lock().unwrap();Ok(c.query_row("SELECT id,title,audio_path,caption,lyrics,metadata_json,generation_settings_json,engine_id,profile_id,replay_request_json,audio_codes_json,source,created_at,updated_at FROM songs WHERE id=?",[id],row_song).optional()?)}
 pub fn create_song(&self,input:SongInput)->Result<Song>{let now=now();let song=Song{id:uuid::Uuid::now_v7().to_string(),title:input.title,audio_path:input.audio_path,caption:input.caption,lyrics:input.lyrics,metadata:input.metadata,generation_settings:input.generation_settings,engine_id:input.engine_id,profile_id:input.profile_id,replay_request:input.replay_request,audio_codes:input.audio_codes,source:input.source,created_at:now.clone(),updated_at:now};self.save_song(&song)?;Ok(song)}
 pub fn import_generated_song(&self,input:GeneratedSongInput)->Result<ImportedSong>{
  if input.audio.is_empty(){anyhow::bail!("cannot import an empty audio result")}
  fs::create_dir_all(&self.media_dir).with_context(||format!("create media directory {}",self.media_dir.display()))?;
  let id=uuid::Uuid::now_v7().to_string();let filename=format!("{id}.{}",input.audio_extension);let target=self.media_dir.join(&filename);let temporary=self.media_dir.join(format!("{filename}.part"));
  {let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&temporary)?;use std::io::Write;file.write_all(&input.audio)?;file.sync_all()?;}
  fs::rename(&temporary,&target).with_context(||format!("publish generated audio {}",target.display()))?;
  let now=now();let title=input.title.map(|t|t.trim().to_owned()).filter(|t|!t.is_empty()).unwrap_or_else(||generated_title(&input.caption));let song=Song{id,title,audio_path:Some(target.display().to_string()),caption:input.caption,lyrics:input.lyrics,metadata:input.metadata,generation_settings:input.generation_settings,engine_id:input.engine_id,profile_id:input.profile_id,replay_request:input.replay_request,audio_codes:input.audio_codes,source:input.source,created_at:now.clone(),updated_at:now};
  if let Err(error)=self.save_song(&song){let _=fs::remove_file(&target);return Err(error.context("store generated song record"));}
  Ok(ImportedSong{song,audio_filename:filename})
 }
 pub fn import_audio_song(&self,input:AudioImportInput)->Result<ImportedSong>{
  if input.audio.is_empty(){anyhow::bail!("cannot import an empty audio file")}
  let extension=input.audio_extension.trim().to_ascii_lowercase();
  if !matches!(extension.as_str(), "mp3" | "wav"){anyhow::bail!("only MP3 and WAV audio can be imported")}
  fs::create_dir_all(&self.media_dir).with_context(||format!("create media directory {}",self.media_dir.display()))?;
  let id=uuid::Uuid::now_v7().to_string(); let filename=format!("{id}.{extension}"); let target=self.media_dir.join(&filename); let temporary=self.media_dir.join(format!("{filename}.part"));
  {let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&temporary)?;use std::io::Write;file.write_all(&input.audio)?;file.sync_all()?;}
  fs::rename(&temporary,&target).with_context(||format!("publish imported audio {}",target.display()))?;
  let now=now(); let song=Song{id,title:input.title,audio_path:Some(target.display().to_string()),caption:input.caption,lyrics:input.lyrics,metadata:input.metadata,generation_settings:input.generation_settings,engine_id:input.engine_id,profile_id:input.profile_id,replay_request:None,audio_codes:None,source:input.source,created_at:now.clone(),updated_at:now};
  if let Err(error)=self.save_song(&song){let _=fs::remove_file(&target);return Err(error.context("store imported song record"));}
  Ok(ImportedSong{song,audio_filename:filename})
 }
 /// Stores a cover image next to the track audio and records its filename in
 /// the song metadata. Covers are a Studio-side concept: the engine never sees
 /// them, so they are stored as plain media rather than in the request record.
 pub fn store_song_cover(&self,id:&str,image:&[u8],media_type:&str)->Result<Song>{
  if image.is_empty(){anyhow::bail!("cannot store an empty cover image")}
  // The declared type is a claim, the magic numbers are the fact. A model that
  // answered with a JPEG had it stored as `image/png`, and the tag inside the
  // mp3 said PNG over JPEG bytes - players that trust the tag showed nothing.
  let media_type=sniff_image_type(image).unwrap_or(media_type);
  let extension=match media_type.trim().to_ascii_lowercase().as_str(){
   "image/png"=>"png","image/jpeg"|"image/jpg"=>"jpg","image/webp"=>"webp",
   other=>anyhow::bail!("unsupported cover media type '{other}'; use PNG, JPEG or WebP"),
  };
  let Some(mut song)=self.get_song(id)? else{anyhow::bail!("song not found")};
  fs::create_dir_all(&self.media_dir).with_context(||format!("create media directory {}",self.media_dir.display()))?;
  let filename=format!("{id}-cover.{extension}");
  let target=self.media_dir.join(&filename);
  let temporary=self.media_dir.join(format!("{filename}.part"));
  {let mut file=fs::OpenOptions::new().create(true).write(true).truncate(true).open(&temporary)?;use std::io::Write;file.write_all(image)?;file.sync_all()?;}
  fs::rename(&temporary,&target).with_context(||format!("publish cover {}",target.display()))?;
  let previous=song.metadata.get("cover_filename").and_then(|v|v.as_str()).map(str::to_owned);
  if song.metadata.is_null(){song.metadata=serde_json::json!({})}
  if let Some(fields)=song.metadata.as_object_mut(){
   fields.insert("cover_filename".into(),serde_json::Value::String(filename.clone()));
   fields.insert("cover_media_type".into(),serde_json::Value::String(format!("image/{}",if extension=="jpg"{"jpeg"}else{extension})));
  }
  song.updated_at=now();
  if let Err(error)=self.save_song(&song){let _=fs::remove_file(&target);return Err(error.context("store song cover record"))}
  if let Some(previous)=previous.filter(|previous|previous!=&filename){let _=fs::remove_file(self.media_dir.join(previous));}
  self.touch_sessions_with(id)?;
  Ok(song)
 }
 pub fn cover_path_for_song(&self,song:&Song)->Option<(PathBuf,String)>{
  let filename=song.metadata.get("cover_filename")?.as_str()?;
  let media_type=song.metadata.get("cover_media_type").and_then(|v|v.as_str()).unwrap_or("image/png").to_owned();
  let path=self.media_file(filename)?;
  // Covers stored before the type was read off the bytes carry the wrong one
  // in their record. The file itself still says what it is.
  let media_type=Self::read_head(&path).and_then(|head|sniff_image_type(&head)).map(str::to_owned).unwrap_or(media_type);
  Some((path,media_type))
 }
 fn read_head(path:&std::path::Path)->Option<Vec<u8>>{
  use std::io::Read;
  let mut file=fs::File::open(path).ok()?;
  let mut head=[0u8;16];
  let read=file.read(&mut head).ok()?;
  Some(head[..read].to_vec())
 }
 /// Where media for this library lives. Stems are written here so they
 /// sit beside the track they came from.
 pub fn media_dir(&self)->&Path{&self.media_dir}
 pub fn media_file(&self,filename:&str)->Option<PathBuf>{if filename.is_empty()||filename.contains(['/', '\\'])||Path::new(filename).file_name().and_then(|x|x.to_str())!=Some(filename){return None}let path=self.media_dir.join(filename);path.is_file().then_some(path)}
 /// Where a stored audio path is now. Paths are stored whole, so a library whose
 /// folder moved - a portable copied elsewhere, a drive back under another
 /// letter - finds its files by name in its own media folder; nothing outside
 /// that folder is ever served.
 pub fn resolve_media(&self,stored:&str)->Option<PathBuf>{
  let root=self.media_dir.canonicalize().ok()?;
  let stored=PathBuf::from(stored);
  if let Ok(path)=stored.canonicalize(){if path.starts_with(&root){return Some(path)}}
  let by_name=self.media_dir.join(stored.file_name()?).canonicalize().ok()?;
  by_name.starts_with(&root).then_some(by_name)
 }
 pub fn media_path_for_song(&self,song:&Song)->Option<PathBuf>{self.resolve_media(song.audio_path.as_ref()?)}
 /// Stores karaoke timings with the track. They live in the metadata rather
 /// than a column of their own so an existing library needs no migration, and
 /// a track without them is simply a track nobody has timed yet.
 pub fn set_song_lrc(&self,id:&str,lrc:&str)->Result<Option<Song>>{
  let Some(mut song)=self.get_song(id)? else{return Ok(None)};
  let mut metadata=match song.metadata.take(){serde_json::Value::Object(map)=>map,_=>serde_json::Map::new()};
  if lrc.trim().is_empty(){metadata.remove("lrc");}else{metadata.insert("lrc".into(),serde_json::Value::String(lrc.to_owned()));}
  song.metadata=serde_json::Value::Object(metadata);
  song.updated_at=now();
  self.save_song(&song)?;
  self.touch_sessions_with(id)?;
  Ok(Some(song))
 }
 /// The thumbs-up of the library, kept in the song's own metadata (like the
 /// karaoke timings) so a like survives a profile or machine change instead of
 /// living in one window's local storage. Taking it back removes the mark.
 pub fn set_song_liked(&self,id:&str,liked:bool)->Result<Option<Song>>{
  let Some(mut song)=self.get_song(id)? else{return Ok(None)};
  let mut metadata=match song.metadata.take(){serde_json::Value::Object(map)=>map,_=>serde_json::Map::new()};
  if liked{
   metadata.insert("liked".into(),serde_json::Value::Bool(true));
   // When the person last reacted to this track: a like moves it, nothing else does -
   // a renamed title or a new cover is an edit, not a reaction.
   metadata.insert("liked_at".into(),serde_json::Value::String(now()));
  }else{
   metadata.remove("liked");
   metadata.remove("liked_at");
  }
  song.metadata=serde_json::Value::Object(metadata);
  song.updated_at=now();
  self.save_song(&song)?;
  Ok(Some(song))
 }
 /// The songs marked as the best ones, newest first: what the library's
 /// thumbs-up list shows and what library_liked answers with.
 pub fn liked_song_ids(&self)->Result<Vec<String>>{
  Ok(self.list_songs()?.into_iter()
   .filter(|song|song.metadata.get("liked").and_then(serde_json::Value::as_bool).unwrap_or(false))
   .map(|song|song.id).collect())
 }
 /// Adds a processed version of a track, stored in the media folder, and makes
 /// it the one that plays. The original's file is remembered on the first
 /// version, so a track can always go back to what it was generated as. Kept in
 /// the metadata, like the karaoke timings, so no migration is needed.
 pub fn select_song_version(&self,id:&str,version:&str)->Result<Option<Song>>{
  let Some(mut song)=self.get_song(id)? else{return Ok(None)};
  let original=song.metadata.get("original_audio_path").and_then(|v|v.as_str()).map(str::to_owned);
  let audio=if version=="original"{
   let original=original.context("this track has no other version")?;
   self.resolve_media(&original).context("the original's file is missing")?.to_string_lossy().into_owned()
  }else{
   let file=song.metadata.get("audio_versions").and_then(|v|v.as_array()).and_then(|list|list.iter().find(|v|v.get("id").and_then(|x|x.as_str())==Some(version))).and_then(|v|v.get("file")).and_then(|v|v.as_str()).map(str::to_owned).context("no such version")?;
   self.media_file(&file).context("the version's file is missing")?.to_string_lossy().into_owned()
  };
  if let Some(fields)=song.metadata.as_object_mut(){fields.insert("active_version".into(),serde_json::Value::String(version.to_owned()));}
  song.audio_path=Some(audio);
  song.updated_at=now();
  self.save_song(&song)?;
  self.touch_sessions_with(id)?;
  Ok(Some(song))
 }
 /// Forgets a version; when it was playing, the original plays again.
 /// Returns the file to remove.
 pub fn remove_song_version(&self,id:&str,version:&str)->Result<Option<(Song,Option<PathBuf>)>>{
  let Some(song)=self.get_song(id)? else{return Ok(None)};
  let active=song.metadata.get("active_version").and_then(|v|v.as_str())==Some(version);
  let mut song=if active{self.select_song_version(id,"original")?.context("the track went away")?}else{song};
  let mut file=None;
  if let Some(list)=song.metadata.get_mut("audio_versions").and_then(|v|v.as_array_mut()){
   if let Some(index)=list.iter().position(|v|v.get("id").and_then(|x|x.as_str())==Some(version)){
    file=list[index].get("file").and_then(|v|v.as_str()).and_then(|name|self.media_file(name));
    list.remove(index);
   }
  }
  song.updated_at=now();
  self.save_song(&song)?;
  self.touch_sessions_with(id)?;
  Ok(Some((song,file)))
 }
 pub fn update_song(&self,id:&str,input:SongInput)->Result<Option<Song>>{let Some(mut song)=self.get_song(id)? else{return Ok(None)};song.title=input.title;if input.audio_path.is_some(){song.audio_path=input.audio_path};song.caption=input.caption;song.lyrics=input.lyrics;song.metadata=merged_metadata(&song.metadata,input.metadata);song.generation_settings=input.generation_settings;song.engine_id=input.engine_id;song.profile_id=input.profile_id;if input.replay_request.is_some(){song.replay_request=input.replay_request};if input.audio_codes.is_some(){song.audio_codes=input.audio_codes};song.source=input.source;song.updated_at=now();self.save_song(&song)?;self.touch_sessions_with(id)?;Ok(Some(song))}
 pub fn delete_song(&self,id:&str)->Result<bool>{self.touch_sessions_with(id)?;let gone=self.connection.lock().unwrap().execute("DELETE FROM songs WHERE id=?",[id])?>0;if gone{crate::mcp::announce("library")}Ok(gone)}
 /// Puts a stem away without losing it: the row keeps the song it was separated from,
 /// so a later separation can put a new set in its place and nothing is forgotten.
 pub fn archive_stem(&self,row:&ArchivedStem)->Result<()>{
  let c=self.connection.lock().unwrap();
  c.execute("INSERT OR REPLACE INTO stems_archive(id,song_id,song_title,batch,batch_slug,stem,title,audio_path,duration_secs,created_at,archived_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",rusqlite::params![row.id,row.song_id,row.song_title,row.batch,row.batch_slug,row.stem,row.title,row.audio_path,row.duration_secs,row.created_at,row.archived_at])?;
  Ok(())}

 /// The archived stems, of one song or of the whole library, newest first.
 pub fn list_archived_stems(&self,song_id:Option<&str>)->Result<Vec<ArchivedStem>>{
  let c=self.connection.lock().unwrap();
  let sql="SELECT id,song_id,song_title,batch,batch_slug,stem,title,audio_path,duration_secs,created_at,archived_at FROM stems_archive";
  let read=|r:&rusqlite::Row|->rusqlite::Result<ArchivedStem>{Ok(ArchivedStem{id:r.get(0)?,song_id:r.get(1)?,song_title:r.get(2)?,batch:r.get(3)?,batch_slug:r.get(4)?,stem:r.get(5)?,title:r.get(6)?,audio_path:r.get(7)?,duration_secs:r.get(8)?,created_at:r.get(9)?,archived_at:r.get(10)?})};
  match song_id{
   Some(id)=>Ok(c.prepare(&format!("{sql} WHERE song_id=?1 ORDER BY archived_at DESC"))?.query_map([id],read)?.collect::<rusqlite::Result<_>>()?),
   None=>Ok(c.prepare(&format!("{sql} ORDER BY archived_at DESC"))?.query_map([],read)?.collect::<rusqlite::Result<_>>()?)}}

 /// A set of stems that an older studio put away carries the words of one language inside its
 /// label: the word for stems, a dash, then the moment - all spelled in that language's letters.
 /// The library keeps moments, not words, so the label is written again from the moment the set
 /// was made (the window dresses it in the person's language). A library that lived through the
 /// change reads the same as a new one, and a label that already holds a moment is left alone.
 fn read_sets_of_stems_without_words(&self)->Result<()>{
  let c=self.connection.lock().unwrap();
  let rows:Vec<(String,String,String)>={
   let mut statement=c.prepare("SELECT id,created_at,batch FROM stems_archive")?;
   statement.query_map([],|r:&rusqlite::Row|->rusqlite::Result<(String,String,String)>{Ok((r.get(0)?,r.get(1)?,r.get(2)?))})?.collect::<rusqlite::Result<_>>()?};
  for (id,created_at,batch) in rows{
   let moment=crate::stem_batch_stamp(&created_at);
   if moment!=batch{c.execute("UPDATE stems_archive SET batch=?1 WHERE id=?2",rusqlite::params![moment,id])?;}}
  Ok(())}

 fn save_song(&self,s:&Song)->Result<()> {self.connection.lock().unwrap().execute("INSERT INTO songs VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title,audio_path=excluded.audio_path,caption=excluded.caption,lyrics=excluded.lyrics,metadata_json=excluded.metadata_json,generation_settings_json=excluded.generation_settings_json,engine_id=excluded.engine_id,profile_id=excluded.profile_id,replay_request_json=excluded.replay_request_json,audio_codes_json=excluded.audio_codes_json,source=excluded.source,updated_at=excluded.updated_at",params![s.id,s.title,s.audio_path,s.caption,s.lyrics,s.metadata.to_string(),s.generation_settings.to_string(),s.engine_id,s.profile_id,s.replay_request.as_ref().map(|v|v.to_string()),s.audio_codes.as_ref().map(|v|v.to_string()),s.source,s.created_at,s.updated_at])?;crate::mcp::announce("library");Ok(())}
 pub fn list_playlists(&self)->Result<Vec<Playlist>>{let c=self.connection.lock().unwrap();let mut s=c.prepare("SELECT id,name,description,created_at,updated_at FROM playlists ORDER BY created_at DESC")?;Ok(s.query_map([],|r|playlist(&c,r))?.collect::<rusqlite::Result<_>>()?)}
 pub fn get_playlist(&self,id:&str)->Result<Option<Playlist>>{let c=self.connection.lock().unwrap();c.query_row("SELECT id,name,description,created_at,updated_at FROM playlists WHERE id=?",[id],|r|playlist(&c,r)).optional().map_err(Into::into)}
 pub fn create_playlist(&self,input:PlaylistInput)->Result<Playlist>{let now=now();let p=Playlist{id:uuid::Uuid::now_v7().to_string(),name:input.name,description:input.description,song_ids:input.song_ids,created_at:now.clone(),updated_at:now};self.save_playlist(&p)?;Ok(p)}
 pub fn update_playlist(&self,id:&str,input:PlaylistInput)->Result<Option<Playlist>>{let Some(mut playlist)=self.get_playlist(id)? else{return Ok(None)};playlist.name=input.name;playlist.description=input.description;playlist.song_ids=input.song_ids;playlist.updated_at=now();self.save_playlist(&playlist)?;Ok(Some(playlist))}
 pub fn delete_playlist(&self,id:&str)->Result<bool>{Ok(self.connection.lock().unwrap().execute("DELETE FROM playlists WHERE id=?",[id])?>0)}
 fn save_playlist(&self,p:&Playlist)->Result<()> {let mut c=self.connection.lock().unwrap();let tx=c.transaction()?;tx.execute("INSERT INTO playlists VALUES(?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,updated_at=excluded.updated_at",params![p.id,p.name,p.description,p.created_at,p.updated_at])?;tx.execute("DELETE FROM playlist_songs WHERE playlist_id=?",[&p.id])?;for(pos,id)in p.song_ids.iter().enumerate(){tx.execute("INSERT INTO playlist_songs VALUES(?,?,?)",params![p.id,id,pos as i64])?;}tx.commit()?;Ok(())}
 pub fn list_workspaces(&self)->Result<Vec<Workspace>>{let c=self.connection.lock().unwrap();let mut s=c.prepare("SELECT id,name,opened_at,closed_at,created_at,updated_at FROM workspaces ORDER BY created_at DESC")?;Ok(s.query_map([],|r|workspace(&c,r))?.collect::<rusqlite::Result<_>>()?)}
 pub fn get_workspace(&self,id:&str)->Result<Option<Workspace>>{let c=self.connection.lock().unwrap();c.query_row("SELECT id,name,opened_at,closed_at,created_at,updated_at FROM workspaces WHERE id=?",[id],|r|workspace(&c,r)).optional().map_err(Into::into)}
 /// The workspace the studio works in right now, if any. Only one is ever open:
 /// the session on screen is the one a new song lands in.
 pub fn active_workspace(&self)->Result<Option<Workspace>>{let c=self.connection.lock().unwrap();c.query_row("SELECT id,name,opened_at,closed_at,created_at,updated_at FROM workspaces WHERE closed_at IS NULL ORDER BY opened_at DESC LIMIT 1",[],|r|workspace(&c,r)).optional().map_err(Into::into)}
 /// A new workspace is created closed: it becomes the current session only when
 /// it is opened. Leaving `closed_at` empty made the first session to arrive
 /// look like the open one, and the studio then thought it was already working.
 /// Whether the agent may work inside this session without being asked: the person
 /// answers "in this session" once, and the answer is kept with the session itself.
 pub fn session_agent_allow(&self,id:&str)->Result<bool>{let c=self.connection.lock().unwrap();Ok(c.query_row("SELECT allow FROM session_agent WHERE session_id=?",[id],|r|r.get::<_,i64>(0)).optional()?.unwrap_or(0)!=0)}
 /// Remember, or take back, that answer. It lives and dies with the session: the row
 /// points at the session and goes when the session goes.
 pub fn set_session_agent_allow(&self,id:&str,allow:bool)->Result<bool>{let now=now();self.connection.lock().unwrap().execute("INSERT INTO session_agent VALUES(?,?,?) ON CONFLICT(session_id) DO UPDATE SET allow=excluded.allow,updated_at=excluded.updated_at",params![id,if allow{1}else{0},now])?;self.session_agent_allow(id)}
 /// Whether the agent may make tracks without asking: one switch for the whole
 /// studio, kept among the system settings, the way the leash is.
 pub fn agent_tracks(&self)->Result<bool>{Ok(self.agent_setting("tracks").unwrap_or_default()=="free")}
 pub fn set_agent_tracks(&self,free:bool)->Result<bool>{self.set_agent_setting("tracks",if free{"free"}else{"ask"})?;self.agent_tracks()}
 /// Tools the agent may never run, whatever it asks: the person's "never allow",
 /// kept as a list among the system settings.
 pub fn agent_forbidden(&self)->Result<Vec<String>>{Ok(self.agent_setting("forbidden").unwrap_or_default().split('\n').map(str::trim).filter(|name|!name.is_empty()).map(str::to_string).collect())}
 pub fn set_agent_forbidden(&self,tools:&[String])->Result<Vec<String>>{self.set_agent_setting("forbidden",&tools.join("\n"))?;self.agent_forbidden()}
 /// How close the agent is kept: `free` (no questions), `risky` (ask about what
 /// cannot be undone) or `all` (ask about everything). `risky` is the default.
 pub fn agent_leash(&self)->Result<String>{let leash=self.agent_setting("leash")?;Ok(if leash.is_empty(){"risky".into()}else{leash})}
 pub fn set_agent_leash(&self,value:&str)->Result<String>{self.set_agent_setting("leash",value)?;self.agent_leash()}
 /// Where the agent puts the tracks it makes, once the person has said it for good:
 /// `current` (the session open at the time) or `each` (a session for every track).
 /// Empty means nothing is settled and the studio asks for each pack of work.
 pub fn agent_session_choice(&self)->Result<String>{self.agent_setting("session_choice")}
 pub fn set_agent_session_choice(&self,value:&str)->Result<String>{self.set_agent_setting("session_choice",value)?;self.agent_session_choice()}
 fn agent_setting(&self,key:&str)->Result<String>{let c=self.connection.lock().unwrap();Ok(c.query_row("SELECT value FROM agent_settings WHERE key=?",[key],|r|r.get::<_,String>(0)).optional()?.unwrap_or_default())}
 fn set_agent_setting(&self,key:&str,value:&str)->Result<()>{let now=now();self.connection.lock().unwrap().execute("INSERT INTO agent_settings VALUES(?,?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",params![key,value,now])?;Ok(())}
 pub fn create_workspace(&self,input:WorkspaceInput)->Result<Workspace>{let now=now();let w=Workspace{id:uuid::Uuid::now_v7().to_string(),name:input.name,song_ids:input.song_ids,opened_at:None,closed_at:Some(now.clone()),created_at:now.clone(),updated_at:now,agent_allow:false,kind:None};self.save_workspace(&w)?;Ok(w)}
 /// Puts one song into a session: work someone did belongs to the session it was made in,
 /// otherwise the track lands in the library behind the open session and looks lost.
 pub fn add_song_to_workspace(&self,workspace_id:&str,song_id:&str)->Result<bool>{
  let Some(mut w)=self.get_workspace(workspace_id)? else{return Ok(false)};
  if !w.song_ids.iter().any(|id|id==song_id){w.song_ids.push(song_id.to_string());w.updated_at=now();self.save_workspace(&w)?}
  Ok(true)}
 /// A session is changed when what it holds changes - a track added or taken out, or a
 /// track inside it edited. A thumbs-up is a reaction, not a change, so it is left out.
 fn touch_sessions_with(&self,song_id:&str)->Result<()>{
  let c=self.connection.lock().unwrap();
  c.execute("UPDATE workspaces SET updated_at=? WHERE id IN (SELECT workspace_id FROM workspace_songs WHERE song_id=?)",params![now(),song_id])?;
  Ok(())}

 pub fn update_workspace(&self,id:&str,input:WorkspaceInput)->Result<Option<Workspace>>{let Some(mut w)=self.get_workspace(id)? else{return Ok(None)};w.name=input.name;w.song_ids=input.song_ids;w.updated_at=now();self.save_workspace(&w)?;
  // A name a person typed makes the session their own: the studio's mark goes, so the window
  // shows their words instead of its own for a session it keeps. Gathering again, if it is
  // ever needed, makes a fresh one - a session someone named is not a place to put strangers.
  if !w.name.trim().is_empty(){self.connection.lock().unwrap().execute("DELETE FROM session_kinds WHERE session_id=? AND kind='import'",[id])?;w.kind=None;}
  Ok(Some(w))}
 pub fn delete_workspace(&self,id:&str)->Result<bool>{Ok(self.connection.lock().unwrap().execute("DELETE FROM workspaces WHERE id=?",[id])?>0)}
 /// Opening a workspace closes the one that was open before it - the studio
 /// keeps a single session at a time, and the closed one keeps its songs.
 pub fn open_workspace(&self,id:&str)->Result<Option<Workspace>>{let now=now();{let mut c=self.connection.lock().unwrap();let tx=c.transaction()?;tx.execute("UPDATE workspaces SET closed_at=?,updated_at=? WHERE closed_at IS NULL",params![now,now])?;tx.execute("UPDATE workspaces SET opened_at=?,closed_at=NULL,updated_at=? WHERE id=?",params![now,now,id])?;tx.commit()?;}self.get_workspace(id)}
 pub fn close_workspace(&self,id:&str)->Result<Option<Workspace>>{let now=now();{let c=self.connection.lock().unwrap();c.execute("UPDATE workspaces SET closed_at=?,updated_at=? WHERE id=?",params![now,now,id])?;}self.get_workspace(id)}
 /// The session that gathers tracks made outside any session. Which one it is is kept in
 /// `session_kinds`, never guessed from a name: the name a person reads is drawn by the
 /// window in its own language, so two windows used to make two such sessions, each with
 /// its own idea of which tracks had no home. `known` carries the spellings a window may
 /// have used before this rule existed, so an older library adopts the session it has.
 pub fn adopt_import_workspace(&self,known:&[String])->Result<Workspace>{
  let id={let now=now();let mut c=self.connection.lock().unwrap();let tx=c.transaction()?;
   let marked:Option<String>=tx.query_row("SELECT session_id FROM session_kinds WHERE kind='import' ORDER BY created_at LIMIT 1",[],|r|r.get(0)).optional()?;
   let id=match marked{Some(id)=>id,None=>{
    let adopted:Option<String>=if known.is_empty(){None}else{
     let placeholders=known.iter().map(|_|"?").collect::<Vec<_>>().join(",");
     let mut q=tx.prepare(&format!("SELECT id FROM workspaces WHERE name IN ({placeholders}) ORDER BY created_at LIMIT 1"))?;
     q.query_row(rusqlite::params_from_iter(known.iter()),|r|r.get::<_,String>(0)).optional()?};
    let id=match adopted{
     Some(id)=>{tx.execute("UPDATE workspaces SET name='',updated_at=? WHERE id=?",params![now,id])?;id}
     None=>{let id=uuid::Uuid::now_v7().to_string();tx.execute("INSERT INTO workspaces VALUES(?,?,?,?,?,?)",params![id,String::new(),now,now,Option::<String>::None,Some(now.clone())])?;id}};
    tx.execute("INSERT INTO session_kinds VALUES(?,?,?)",params![id,"import",now])?;id}};
   let orphans:Vec<String>={let mut q=tx.prepare("SELECT id FROM songs WHERE id NOT IN (SELECT song_id FROM workspace_songs) ORDER BY created_at")?;q.query_map([],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?};
   let mut position:i64=tx.query_row("SELECT COALESCE(MAX(position),-1) FROM workspace_songs WHERE workspace_id=?",[&id],|r|r.get(0))?;
   for song in &orphans{position+=1;tx.execute("INSERT OR IGNORE INTO workspace_songs VALUES(?,?,?)",params![id,song,position])?;}
   tx.commit()?;id};
  self.get_workspace(&id)?.ok_or_else(||anyhow::anyhow!("the import session is gone"))
 }
 fn save_workspace(&self,w:&Workspace)->Result<()> {let mut c=self.connection.lock().unwrap();let tx=c.transaction()?;tx.execute("INSERT INTO workspaces VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,opened_at=excluded.opened_at,closed_at=excluded.closed_at,updated_at=excluded.updated_at",params![w.id,w.name,w.created_at,w.updated_at,w.opened_at,w.closed_at])?;tx.execute("DELETE FROM workspace_songs WHERE workspace_id=?",[&w.id])?;for(pos,id)in w.song_ids.iter().enumerate(){tx.execute("INSERT INTO workspace_songs VALUES(?,?,?)",params![w.id,id,pos as i64])?;}tx.commit()?;Ok(())}
 /// How a context plays back, or nothing when it was never touched. What a
 /// fresh context defaults to is the caller's business; this only remembers.
 pub fn playback_mode(&self,context_key:&str)->Result<Option<PlaybackMode>>{let c=self.connection.lock().unwrap();c.query_row("SELECT repeat_mode,shuffle,updated_at FROM playback_modes WHERE context_key=?",[context_key],|r|Ok(PlaybackMode{repeat_mode:r.get(0)?,shuffle:r.get(1)?,updated_at:r.get(2)?})).optional().map_err(Into::into)}
 pub fn set_playback_mode(&self,context_key:&str,input:PlaybackModeInput)->Result<PlaybackMode>{let updated_at=now();let c=self.connection.lock().unwrap();c.execute("INSERT INTO playback_modes VALUES(?,?,?,?) ON CONFLICT(context_key) DO UPDATE SET repeat_mode=excluded.repeat_mode,shuffle=excluded.shuffle,updated_at=excluded.updated_at",params![context_key,input.repeat_mode,input.shuffle,updated_at])?;Ok(PlaybackMode{repeat_mode:input.repeat_mode,shuffle:input.shuffle,updated_at})}
}
/// An edit's metadata over the stored one: what the studio keeps there itself
/// (processed versions, the original audio, karaoke, the cover) is not the
/// client's to drop by leaving it out.
fn merged_metadata(stored:&serde_json::Value,edit:serde_json::Value)->serde_json::Value{
 let(Some(stored),serde_json::Value::Object(mut edit))=(stored.as_object(),edit.clone()) else{return edit};
 for(key,value)in stored{edit.entry(key.clone()).or_insert_with(||value.clone());}
 serde_json::Value::Object(edit)
}
fn row_song(r:&rusqlite::Row)->rusqlite::Result<Song>{Ok(Song{id:r.get(0)?,title:r.get(1)?,audio_path:r.get(2)?,caption:r.get(3)?,lyrics:r.get(4)?,metadata:json(r.get::<_,String>(5)?),generation_settings:json(r.get::<_,String>(6)?),engine_id:r.get(7)?,profile_id:r.get(8)?,replay_request:r.get::<_,Option<String>>(9)?.map(json),audio_codes:r.get::<_,Option<String>>(10)?.map(json),source:r.get(11)?,created_at:r.get(12)?,updated_at:r.get(13)?})}fn json(s:String)->serde_json::Value{serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)}fn playlist(c:&Connection,r:&rusqlite::Row)->rusqlite::Result<Playlist>{let id:String=r.get(0)?;let mut q=c.prepare("SELECT song_id FROM playlist_songs WHERE playlist_id=? ORDER BY position")?;let song_ids=q.query_map([&id],|x|x.get(0))?.collect::<rusqlite::Result<_>>()?;Ok(Playlist{id,name:r.get(1)?,description:r.get(2)?,song_ids,created_at:r.get(3)?,updated_at:r.get(4)?})}/// A workspace is one session of work: the songs made in it and whether it is
/// still open. A single workspace is open at a time - what the studio calls the
/// current session - so opening one closes whichever was open before it.
#[derive(Debug, Clone, Serialize, Deserialize)] pub struct Workspace { pub id:String, pub name:String, pub song_ids:Vec<String>, pub opened_at:Option<String>, pub closed_at:Option<String>, pub created_at:String, pub updated_at:String,
 /// May the agent work here without asking? It is kept in `session_agent`, with the session.
 #[serde(default)] pub agent_allow:bool,
 /// What the studio calls a session it keeps itself, or nothing for the person's own
 /// sessions: `import` for the one that gathers tracks made outside any session. The
 /// mark is what makes it one session, not a name in some language.
 #[serde(default)] pub kind:Option<String> }
#[derive(Debug, Clone, Deserialize)] pub struct WorkspaceInput { pub name:String, #[serde(default)] pub song_ids:Vec<String> }
/// How one queue plays back. The repeat mode and shuffle belong to the context
/// they were chosen in - a workspace, a playlist, the library, a search - and
/// are kept apart from the others, so each one comes back the way it was left.
#[derive(Debug, Clone, Serialize, Deserialize)] pub struct PlaybackMode { pub repeat_mode:String, pub shuffle:bool, pub updated_at:String }

#[derive(Debug, Clone, Deserialize)] pub struct PlaybackModeInput { pub repeat_mode:String, #[serde(default)] pub shuffle:bool }
/// The flag that belongs to a session, read from its own row: the agent sees it when
/// it looks at sessions, and the window shows it on the session.
fn agent_allow_of(c:&Connection,id:&str)->bool{c.query_row("SELECT allow FROM session_agent WHERE session_id=?",[id],|r|r.get::<_,i64>(0)).optional().unwrap_or(Some(0)).unwrap_or(0)!=0}
fn kind_of(c:&Connection,id:&str)->Option<String>{c.query_row("SELECT kind FROM session_kinds WHERE session_id=?",[id],|r|r.get::<_,String>(0)).optional().unwrap_or(None)}fn workspace(c:&Connection,r:&rusqlite::Row)->rusqlite::Result<Workspace>{let id:String=r.get(0)?;let mut q=c.prepare("SELECT song_id FROM workspace_songs WHERE workspace_id=? ORDER BY position")?;let song_ids=q.query_map([&id],|x|x.get(0))?.collect::<rusqlite::Result<_>>()?;Ok({let agent_allow=agent_allow_of(c,&id);let kind=kind_of(c,&id);Workspace{id,name:r.get(1)?,song_ids,opened_at:r.get(2)?,closed_at:r.get(3)?,created_at:r.get(4)?,updated_at:r.get(5)?,agent_allow,kind}})}
fn now()->String{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs().to_string()}
#[cfg(test)]mod tests{use super::*;
#[test]fn a_workspace_keeps_its_songs_and_only_one_stays_open(){let root=std::env::temp_dir().join(format!("yue2-workspace-test-{}",uuid::Uuid::now_v7()));let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();let song=db.create_song(SongInput{title:"A".into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::Value::Null,generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;let first=db.create_workspace(WorkspaceInput{name:"Night".into(),song_ids:vec![song.clone()]}).unwrap();let second=db.create_workspace(WorkspaceInput{name:"Rock".into(),song_ids:vec![]}).unwrap();assert!(db.active_workspace().unwrap().is_none(),"a session that was only created is not open");assert_eq!(db.get_workspace(&first.id).unwrap().unwrap().song_ids,vec![song.clone()]);db.open_workspace(&first.id).unwrap();assert_eq!(db.active_workspace().unwrap().unwrap().id,first.id);db.open_workspace(&second.id).unwrap();assert!(db.get_workspace(&first.id).unwrap().unwrap().closed_at.is_some());assert_eq!(db.active_workspace().unwrap().unwrap().id,second.id);db.close_workspace(&second.id).unwrap();assert!(db.active_workspace().unwrap().is_none());assert!(db.delete_workspace(&second.id).unwrap());drop(db);fs::remove_dir_all(&root).unwrap();}
/// Tracks made outside a session are gathered into one session the studio marks itself,
/// and a second look adopts that same one instead of making another.
#[test]fn tracks_made_outside_a_session_are_gathered_once(){
 let root=std::env::temp_dir().join(format!("yue2-import-session-test-{}",uuid::Uuid::now_v7()));
 let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();
 let song=|title:&str| db.create_song(SongInput{title:title.into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::Value::Null,generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;
 song("One");song("Two");
 let known=vec!["Импорт".to_string(),"Import".to_string()];
 let first=db.adopt_import_workspace(&known).unwrap();
 assert_eq!(first.kind.as_deref(),Some("import"),"the studio marks the session, not its name");
 assert_eq!(first.name,"","no language is written into the library");
 assert_eq!(first.song_ids.len(),2,"both tracks are gathered");
 assert!(first.closed_at.is_some(),"gathering does not open a session");
 song("Three");
 let again=db.adopt_import_workspace(&known).unwrap();
 assert_eq!(again.id,first.id,"a second look adopts the same session");
 assert_eq!(again.song_ids.len(),3,"the track that arrived later joins it too");
 assert_eq!(db.list_workspaces().unwrap().len(),1,"one session, not two");
 let second=db.update_workspace(&first.id,WorkspaceInput{name:"Мой импорт".into(),song_ids:again.song_ids.clone()}).unwrap().unwrap();
 assert_eq!(second.kind,None,"a session a person named is no longer the studio's own");
 let older=Library::open_at(root.join("older.sqlite"),root.join("media")).unwrap();
 let named=older.create_workspace(WorkspaceInput{name:"Импорт".into(),song_ids:vec![]}).unwrap();
 let adopted=older.adopt_import_workspace(&known).unwrap();
 assert_eq!(adopted.id,named.id,"the session that is already there is the one that is used");
 assert_eq!(adopted.kind.as_deref(),Some("import"));
 assert_eq!(older.list_workspaces().unwrap().len(),1,"an older library gains no second one");
 drop((db,older));fs::remove_dir_all(&root).unwrap();
}
#[test]fn a_playback_mode_is_kept_per_context(){let root=std::env::temp_dir().join(format!("yue2-mode-test-{}",uuid::Uuid::now_v7()));let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();db.set_playback_mode("library:all",PlaybackModeInput{repeat_mode:"none".into(),shuffle:false}).unwrap();db.set_playback_mode("session:1",PlaybackModeInput{repeat_mode:"all".into(),shuffle:true}).unwrap();let library=db.playback_mode("library:all").unwrap().unwrap();assert_eq!(library.repeat_mode,"none");assert!(!library.shuffle);let session=db.playback_mode("session:1").unwrap().unwrap();assert_eq!(session.repeat_mode,"all");assert!(session.shuffle);db.set_playback_mode("library:all",PlaybackModeInput{repeat_mode:"one".into(),shuffle:false}).unwrap();assert_eq!(db.playback_mode("library:all").unwrap().unwrap().repeat_mode,"one");assert!(db.playback_mode("nothing").unwrap().is_none());drop(db);fs::remove_dir_all(&root).unwrap();}
#[test]fn an_archived_stem_keeps_the_song_it_came_from(){let root=std::env::temp_dir().join(format!("yue2-stem-archive-test-{}",uuid::Uuid::now_v7()));let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();let song=db.create_song(SongInput{title:"Night".into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::Value::Null,generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;let row=ArchivedStem{id:format!("{}-1",song),song_id:song.clone(),song_title:"Night".into(),batch:"2026-09-27 20:08:03".into(),batch_slug:"STEM-2026-09-27_20-08-03".into(),stem:"bass".into(),title:"Night · bass".into(),audio_path:"night-bass.wav".into(),duration_secs:Some(150.0),created_at:"1790487289".into(),archived_at:"1790487999".into()};db.archive_stem(&row).unwrap();let mine=db.list_archived_stems(Some(&song)).unwrap();assert_eq!(mine.len(),1);assert_eq!(mine[0].stem,"bass","who it is is kept");assert_eq!(mine[0].song_id,song,"it still knows the song it was separated from");assert_eq!(mine[0].batch,"2026-09-27 20:08:03","the set keeps its stamp");assert!(db.list_archived_stems(Some("another-song")).unwrap().is_empty());assert!(db.delete_song(&song).unwrap());assert_eq!(db.list_archived_stems(None).unwrap().len(),1,"an archived stem outlives the song row");drop(db);fs::remove_dir_all(&root).unwrap();}
/// A set of stems keeps the moment it was made - not the letters of a language it was once
/// described in, so a library from an older studio reads the same after the change.
#[test]fn a_set_of_stems_keeps_its_moment_not_the_letters_of_a_language(){let root=std::env::temp_dir().join(format!("yue2-stem-moment-test-{}",uuid::Uuid::now_v7()));let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();let song=db.create_song(SongInput{title:"Night".into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::Value::Null,generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;db.archive_stem(&ArchivedStem{id:format!("{}-1",song),song_id:song.clone(),song_title:"Night".into(),batch:"СТЕМ-2026-09-27 11:34:49".into(),batch_slug:"STEM-2026-09-27_11-34-49".into(),stem:"bass".into(),title:"Night · bass".into(),audio_path:"night-bass.wav".into(),duration_secs:Some(150.0),created_at:"1790487289".into(),archived_at:"1790487999".into()}).unwrap();drop(db);let reopened=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();let mine=reopened.list_archived_stems(Some(&song)).unwrap();assert_eq!(mine.len(),1,"the set is still one set");assert_eq!(mine[0].batch,crate::stem_batch_stamp("1790487289"),"the label is the moment the set was made");assert_eq!(mine[0].batch.len(),19,"a moment reads as a plain date and time");assert!(!mine[0].batch.chars().any(|ch|ch.is_alphabetic()&&!ch.is_ascii()),"no letter outside the plain alphabet is left");drop(reopened);fs::remove_dir_all(&root).unwrap();}
/// The person's answers live with what they are about: "in this session" with the
/// session itself, the studio-wide switches among the system settings, and a tool
/// that is forbidden stays forbidden however the agent asks.
#[test]fn the_persons_answers_are_kept_with_what_they_are_about(){
    let root=std::env::temp_dir().join(format!("yue2-agent-answers-{}",uuid::Uuid::now_v7()));
    let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();
    assert_eq!(db.agent_leash().unwrap(),"risky","спрашивать по опасному - дефолт");
    assert!(!db.agent_tracks().unwrap(),"делать треки без спроса по умолчанию нельзя");
    assert!(db.agent_forbidden().unwrap().is_empty(),"свежая студия ничего не запрещает");
    let session=db.create_workspace(WorkspaceInput{name:"Проба".into(),song_ids:vec![]}).unwrap().id;
    assert!(!db.session_agent_allow(&session).unwrap(),"новая сессия ничего не разрешает");
    assert!(db.set_session_agent_allow(&session,true).unwrap());
    assert!(db.get_workspace(&session).unwrap().unwrap().agent_allow,"сессия несёт свой флаг");
    assert!(db.list_workspaces().unwrap().iter().any(|w|w.id==session&&w.agent_allow),"флаг виден и в списке");
    assert!(db.set_agent_tracks(true).unwrap());
    assert!(db.agent_tracks().unwrap(),"переключатель треков читается обратно");
    assert_eq!(db.set_agent_forbidden(&["library_song_delete".into()]).unwrap(),vec!["library_song_delete".to_string()]);
    assert_eq!(db.agent_forbidden().unwrap(),vec!["library_song_delete".to_string()]);
    assert_eq!(db.set_agent_leash("free").unwrap(),"free","поводок меняется и читается");
    db.delete_workspace(&session).unwrap();
    assert!(!db.session_agent_allow(&session).unwrap(),"флаг уходит вместе с сессией");
    assert_eq!(db.agent_forbidden().unwrap().len(),1,"запрет остаётся, он о студии, а не о сессии");
    drop(db);
    fs::remove_dir_all(&root).unwrap();
}
#[test]fn a_like_is_kept_with_the_song_and_can_be_taken_back(){let root=std::env::temp_dir().join(format!("yue2-liked-test-{}",uuid::Uuid::now_v7()));let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();let song=db.create_song(SongInput{title:"A".into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::json!({"tags":["yue2"]}),generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;assert!(db.liked_song_ids().unwrap().is_empty(),"a fresh library has nothing liked");db.set_song_liked(&song,true).unwrap();assert_eq!(db.liked_song_ids().unwrap(),vec![song.clone()]);assert_eq!(db.get_song(&song).unwrap().unwrap().metadata["liked"],serde_json::json!(true));assert_eq!(db.get_song(&song).unwrap().unwrap().metadata["tags"][0],"yue2","the rest of the metadata survives");db.set_song_liked(&song,false).unwrap();assert!(db.liked_song_ids().unwrap().is_empty());assert!(db.get_song(&song).unwrap().unwrap().metadata.get("liked").is_none());drop(db);fs::remove_dir_all(&root).unwrap();}
#[test]fn a_session_keeps_the_day_its_tracks_changed_but_a_like_is_only_a_reaction(){
 let root=std::env::temp_dir().join(format!("yue2-session-changed-test-{}",uuid::Uuid::now_v7()));
 let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();
 let song=db.create_song(SongInput{title:"A".into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::Value::Null,generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;
 let session=db.create_workspace(WorkspaceInput{name:"Night".into(),song_ids:vec![song.clone()]}).unwrap();
 let made=db.get_workspace(&session.id).unwrap().unwrap().updated_at;
 std::thread::sleep(std::time::Duration::from_millis(1100));
 db.set_song_liked(&song,true).unwrap();
 assert_eq!(db.get_workspace(&session.id).unwrap().unwrap().updated_at,made,"a like is a reaction, not a change of the session");
 let liked=db.get_song(&song).unwrap().unwrap();
 assert!(liked.metadata.get("liked_at").is_some(),"the moment of the reaction is kept with the song");
 assert!(liked.metadata["liked_at"].as_str().is_some_and(|at|!at.is_empty()),"and it is a moment, not an empty word");
 std::thread::sleep(std::time::Duration::from_millis(1100));
 db.set_song_lrc(&song,"[00:01.00] a word").unwrap();
 let edited=db.get_workspace(&session.id).unwrap().unwrap().updated_at;
 assert_ne!(edited,made,"editing a track inside a session changes the session");
 std::thread::sleep(std::time::Duration::from_millis(1100));
 assert!(db.delete_song(&song).unwrap());
 assert_ne!(db.get_workspace(&session.id).unwrap().unwrap().updated_at,edited,"and so does taking the track away");
 drop(db);fs::remove_dir_all(&root).unwrap();}
#[test]fn a_structured_caption_does_not_become_a_title_of_headings(){
  let caption="Global Metadata
Basic Attributes: bpm is 118. key is F# minor, and scale is minor. Darkwave, Synth-pop. Global Emotional Progression: haunting.";
  assert_eq!(generated_title(caption),"Darkwave, Synth-pop");
  assert_eq!(generated_title("Bright uplifting synth-pop, punchy drums"),"Bright uplifting synth-pop, punchy drums");
 }
#[test]fn imports_audio_and_full_provenance_atomically(){let root=std::env::temp_dir().join(format!("yue2-library-test-{}",uuid::Uuid::now_v7()));let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();let result=db.import_generated_song(GeneratedSongInput{title:None,metadata:serde_json::Value::Null,caption:"c".into(),lyrics:"l".into(),generation_settings:serde_json::json!({"seed":1}),replay_request:Some(serde_json::json!({"audio_codes":"1,2,3,4,5,6,7,8","seed":1})),audio_codes:Some(serde_json::json!("1,2,3,4,5,6,7,8")),engine_id:"mm".into(),profile_id:Some("recommended-light".into()),source:"local_generation".into(),audio_extension:"mp3",audio:b"ID3".to_vec()}).unwrap();assert_eq!(db.get_song(&result.song.id).unwrap().unwrap().audio_codes,Some(serde_json::json!("1,2,3,4,5,6,7,8")));assert_eq!(fs::read(db.media_file(&result.audio_filename).unwrap()).unwrap(),b"ID3");drop(db);fs::remove_dir_all(&root).unwrap();}
#[test]fn playlist_can_be_read_updated_and_deleted(){let root=std::env::temp_dir().join(format!("yue2-playlist-test-{}",uuid::Uuid::now_v7()));let db=Library::open_at(root.join("library.sqlite"),root.join("media")).unwrap();let song_a=db.create_song(SongInput{title:"A".into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::Value::Null,generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;let song_b=db.create_song(SongInput{title:"B".into(),audio_path:None,caption:String::new(),lyrics:String::new(),metadata:serde_json::Value::Null,generation_settings:serde_json::Value::Null,engine_id:"manual".into(),profile_id:None,replay_request:None,audio_codes:None,source:"manual".into()}).unwrap().id;let created=db.create_playlist(PlaylistInput{name:"Drafts".into(),description:None,song_ids:vec![song_a]}).unwrap();let updated=db.update_playlist(&created.id,PlaylistInput{name:"Finished".into(),description:Some("native".into()),song_ids:vec![song_b.clone()]}).unwrap().unwrap();assert_eq!(updated.name,"Finished");assert_eq!(db.get_playlist(&created.id).unwrap().unwrap().song_ids,vec![song_b]);assert!(db.delete_playlist(&created.id).unwrap());assert!(db.get_playlist(&created.id).unwrap().is_none());drop(db);fs::remove_dir_all(&root).unwrap();}
#[test]fn measures_wav_duration_from_the_header_and_mp3_from_its_bitrate(){
 // 1 second of 44.1 kHz stereo 16-bit PCM.
 let mut wav=Vec::new();
 wav.extend_from_slice(b"RIFF"); wav.extend_from_slice(&0u32.to_le_bytes()); wav.extend_from_slice(b"WAVE");
 wav.extend_from_slice(b"fmt "); wav.extend_from_slice(&16u32.to_le_bytes());
 wav.extend_from_slice(&1u16.to_le_bytes()); wav.extend_from_slice(&2u16.to_le_bytes());
 wav.extend_from_slice(&44100u32.to_le_bytes()); wav.extend_from_slice(&176400u32.to_le_bytes());
 wav.extend_from_slice(&4u16.to_le_bytes()); wav.extend_from_slice(&16u16.to_le_bytes());
 wav.extend_from_slice(b"data"); wav.extend_from_slice(&176400u32.to_le_bytes());
 wav.resize(wav.len()+176400,0);
 assert!((audio_duration_seconds(&wav,"wav",None).unwrap()-1.0).abs()<0.001);
 // 128 kbps MP3: 16 kB is one second.
 assert!((audio_duration_seconds(&vec![0u8;16000],"mp3",Some(128)).unwrap()-1.0).abs()<0.01);
 assert!(audio_duration_seconds(b"not audio","wav",None).is_none());
}
#[test]fn a_generated_song_gets_a_readable_title_instead_of_the_whole_caption(){
 assert_eq!(generated_title("cinematic synthwave instrumental, 1980s analog synthesizers, warm bassline, soaring lead melody, polished production"),"cinematic synthwave instrumental, 1980s analog");
 assert_eq!(generated_title("Night drive. Wide synths"),"Night drive");
 assert_eq!(generated_title("   "),"Untitled track");
}}
#[cfg(test)]
mod edit_tests {
    use super::*;

    #[test]
    fn a_rename_keeps_the_processed_versions_and_the_original() {
        let stored = serde_json::json!({ "tags": ["a"], "audio_versions": [{ "id": "v1", "file": "v1.wav" }], "original_audio_path": "song.mp3", "active_version": "v1" });
        let merged = merged_metadata(&stored, serde_json::json!({ "tags": ["b"], "bpm": 120 }));
        assert_eq!(merged["tags"], serde_json::json!(["b"]));
        assert_eq!(merged["bpm"], 120);
        assert_eq!(merged["active_version"], "v1");
        assert_eq!(merged["audio_versions"][0]["file"], "v1.wav");
        assert_eq!(merged["original_audio_path"], "song.mp3");
    }
}

#[cfg(test)]
mod media_tests {
    use super::*;

    #[test]
    fn a_library_whose_folder_moved_still_finds_its_songs() {
        let root = std::env::temp_dir().join(format!("library-moved-{}", uuid::Uuid::now_v7().simple()));
        let db = Library::open_at(root.join("library.sqlite"), root.join("media")).unwrap();
        fs::create_dir_all(root.join("media")).unwrap();
        fs::write(root.join("media").join("song.mp3"), b"ID3").unwrap();
        // stored while the portable folder sat on another drive
        let found = db.resolve_media(r"Z:\old place\data\media\song.mp3").unwrap();
        assert_eq!(found, root.join("media").join("song.mp3").canonicalize().unwrap());
        assert!(db.resolve_media(r"Z:\old place\data\media\gone.mp3").is_none());
        fs::write(root.join("outside.mp3"), b"ID3").unwrap();
        assert!(db.resolve_media(&root.join("outside.mp3").to_string_lossy()).is_none());
        let _ = fs::remove_dir_all(root);
    }
}
