//! Suno request and response schemas, grouped by endpoint domain.

mod account;
mod clip;
mod clip_info;
mod clip_mutation;
mod custom_model;
mod download;
mod feed;
mod generation;
mod lyrics;
mod lyrics_editor;
mod lyrics_project;
mod metadata;
mod operations;
mod persona;
mod playlist;
mod prompts;
mod upload;
mod visual;
mod voice;

pub use account::{
    AccessibleFeatures, BillingInfo, DownloadCreditPack, DownloadUsage, MaxLengths, Model,
    RemasterModelInfo,
};
pub use clip::{Clip, ClipActionConfig};
pub use clip_info::{
    ClipAttribution, ClipComments, ClipInfo, ClipInfoSupplementalError, RemixCountResponse,
    SimilarClipsResponse,
};
pub use clip_mutation::{ClipReaction, ClipTrashRequest, SetClipReactionRequest};
pub use custom_model::{
    ArchiveCustomModelRequest, CreateCustomModelRequest, CustomModelCreateResponse,
    PendingCustomModelsResponse,
};
pub(crate) use download::DownloadAuthorizationRequest;
pub use download::DownloadAuthorizationResponse;
pub use feed::{FeedFilters, FeedResponse, FeedV3Request};
pub use generation::{
    ControlSliders, GenerateRequest, GenerateResponse, GenerationResult, LastTagsGeneration,
};
pub use lyrics::{AlignedWord, CowriteLyricsModel, CowriteLyricsResponse};
pub use lyrics_editor::{
    LyricsMashupRequest, LyricsMashupStatus, LyricsMashupSubmission, LyricsRewriteRequest,
    LyricsRewriteResponse, LyricsRewriteResult,
};
pub use lyrics_project::{
    FlushLyricsProjectRequest, FlushLyricsProjectResponse, LyricsProject,
    LyricsProjectTitleRequest, LyricsProjectsPage,
};
pub use metadata::{SetMetadataRequest, SetVisibilityRequest};
pub use operations::{ConcatRequest, RemasterVariation};
pub use persona::{
    CreatePersonaRequest, EditPersonaRequest, PersonaClipsResponse, PersonaInfo,
    PersonaListResponse, PersonaListScope,
};
pub use persona::{TogglePersonaLoveResponse, TrashPersonasResponse};
pub use playlist::{
    CreatePlaylistRequest, PlaylistInfo, PlaylistListResponse, PlaylistReaction,
    PlaylistReorderRequest, PlaylistTrackMutationFailure, PlaylistTrackMutationReport,
    PlaylistTracksRequest, SetPlaylistCoverRequest, SetPlaylistMetadataRequest,
    SetPlaylistMetadataV2Request, SetPlaylistReactionRequest, SetPlaylistVisibilityRequest,
    TrashPlaylistRequest,
};
pub use prompts::{PromptUpsampleRequest, PromptUpsampleResponse};
pub use upload::{
    AudioUploadInitResponse, AudioUploadStatus, CreateAudioUploadRequest, CreateAudioUploadSpec,
    CreateImageUploadRequest, FinishAudioUploadRequest, FinishImageUploadResponse,
    ImageUploadInitResponse, InitializeAudioClipRequest, InitializeAudioClipResponse,
};
pub use visual::{
    CoverArtApplyResponse, CoverArtBatchDescriptor, CoverArtBatchSubmission, CoverArtCost,
    CoverArtHistoryRequest, CoverArtHistoryResponse, CoverArtImageGenerateRequest,
    CoverArtModelCategory, CoverArtModelConfigs, CoverArtPendingBatches, CoverArtPollResponse,
    CoverArtPromptImage, CoverArtVideoGenerateRequest, PromptImageRequest, PromptImageResponse,
    VideoGenerationStatus,
};
pub use voice::{
    CreateVoiceVerificationRequest, ProcessVoiceSampleRequest, ProcessVoiceSampleResponse,
    ProcessVoiceVerificationRecordingRequest, ProcessVoiceVerificationRecordingResponse,
    ProcessedVoiceStatus, VoicePhrase, VoiceVerification,
};
