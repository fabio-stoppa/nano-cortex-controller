#include "nano_fx.h"

#include <string.h>

static const char * const s_params_0[] = { "Gain", "Tone", "Volume" };

static const char * const s_params_1[] = { "Gain", "Level" };

static const char * const s_params_2[] = { "Gain", "Bass", "Treble", "Volume" };

static const char * const s_params_3[] = { "Gain", "Bass", "Treble", "Volume" };

static const char * const s_params_4[] = { "Gain", "Bass", "Treble", "Volume" };

static const char * const s_params_5[] = { "Fuzz", "Volume", "Pickup (HB/Single)", "Pickup Level" };

static const char * const s_params_6[] = { "Overdrive", "Tone", "Level" };

static const char * const s_params_7[] = { "Drive", "Growl (On/Off)", "Midboost (On/Off)", "Tone", "Level", "Blend" };

static const char * const s_params_8[] = { "Gain", "Volume" };

static const char * const s_params_9[] = { "Drive", "Peak (LP/HP)", "Tone", "Volume" };

static const char * const s_params_10[] = { "Distortion", "Filter", "Volume" };

static const char * const s_params_11[] = { "Noise Reduction" };

static const char * const s_params_12[] = { "65Hz", "125Hz", "250Hz", "500hz", "1kHz", "2kHz", "4kHz", "8kHz", "16kHz", "HPF", "LPF", "Output" };

static const char * const s_params_13[] = { "HPF Slope", "HPF Freq", "LPF Slope", "LPF Freq", "Output" };

static const char * const s_params_14[] = { "1 Gain", "1 Freq", "1 Q", "1 Type (Peak/Hi Pass/Lo Pass/Hi Shelf/Lo Shelf)", "1 Active (On/Off)", "2 Gain", "2 Freq", "2 Q", "2 Type (Peak/Hi Pass/Lo Pass/Hi Shelf/Lo Shelf)", "2 Active (On/Off)", "3 Gain", "3 Freq", "3 Q", "3 Type (Peak/Hi Pass/Lo Pass/Hi Shelf/Lo Shelf)", "3 Active (On/Off)", "Output" };

static const char * const s_params_15[] = { "Threshold", "Attack", "Hold", "Release", "Range" };

static const char * const s_params_16[] = { "Level", "Curve (Linear/Log)" };

static const char * const s_params_17[] = { "Wah" };

static const char * const s_params_18[] = { "Wah" };

static const char * const s_params_19[] = { "Wah" };

static const char * const s_params_20[] = { "Wah" };

static const char * const s_params_21[] = { "Sens", "Attack", "Decay", "LP/BP/HP", "Level", "Freq", "Freq Depth", "Reso", "Mix" };

static const char * const s_params_22[] = { "Sensitivity", "Attack", "Decay", "Color", "Intensity", "Blend", "Trig Detection (Down/Up)", "Trigger Mode", "Filter Cutoff", "Filter Type", "Level" };

static const char * const s_params_23[] = { "Input", "Ratio", "Attack", "Release", "Makeup", "Mix" };

static const char * const s_params_24[] = { "Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix" };

static const char * const s_params_25[] = { "Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix" };

static const char * const s_params_26[] = { "Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix" };

static const char * const s_params_27[] = { "Mix", "Semitones", "Pitch Fine", "High Pass", "Low Pass" };

static const char * const s_params_28[] = { "Spread", "Dry Level", "FX Level" };

static const char * const s_params_29[] = { "Input", "Ratio", "Attack", "Release", "Makeup", "Mix" };

static const char * const s_params_30[] = { "Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix" };

static const char * const s_params_31[] = { "Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix" };

static const char * const s_params_32[] = { "Threshold", "Ratio", "Attack", "Release", "Makeup", "Mix" };

static const char * const s_params_33[] = { "Mix", "Rate", "Depth", "Type", "Width", "Output", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_34[] = { "Mix", "Mode", "Mode Type", "Drive", "Output" };

static const char * const s_params_35[] = { "Mix", "Rate", "Depth", "Width", "Output", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_36[] = { "Mix", "Speed", "Depth", "Mode", "Output", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_37[] = { "Mix", "Manual", "Width", "Speed", "Regen", "Output", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_38[] = { "Mix", "Speed", "Type", "Mode", "Output", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_39[] = { "Mix", "Vibe", "Speed", "Level", "Depth", "Output", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_40[] = { "Rate", "Depth", "Waveform", "Duty Cycle", "Width", "Smoothing", "LFO Active", "Fade In", "Fade Out", "Boost", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_41[] = { "Mix", "Feedback", "High Pass", "Low Pass", "Ping Pong (On/Off)", "Delay Time", "Mod Rate", "Mod Depth", "Width", "Drive", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_42[] = { "Mix", "Tap Preset", "Delay Time", "Feedback", "Diffusion", "High Pass", "Low Pass", "Mod Rate", "Mod Depth", "Vintage Mode (On/Off)", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_43[] = { "Mix", "Feedback", "High Pass", "Low Pass", "Ping Pong (On/Off)", "Delay Time", "Mod Rate", "Mod Depth", "Width", "Dyn Depth", "Dyn Mode", "Threshold", "Attack", "Release", "Knee", "Feedback Depth", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_44[] = { "Mix", "Delay Time L", "Feedback L", "X-Feedback", "Delay Time R", "Feedback R", "High Pass", "Low Pass", "Mod Rate", "Mod Depth", "Link FBack (On/Off)", "Dyn Depth", "Dyn Mode", "Threshold", "Attack", "Release", "Knee", "Feedback Depth", "Sync L (On/Off)", "Sync Note L", "Sync R (On/Off)", "Sync Note R" };

static const char * const s_params_45[] = { "Mix", "Delay Time L", "Feedback L", "X-Feedback", "Feedback Mode", "Delay Time R", "Feedback R", "Overlap", "Trig Threshold", "Dyn Depth", "Dyn Mode", "Threshold", "Attack", "Release", "Knee", "Feedback Depth", "High Pass", "Low Pass", "Link FBack (On/Off)", "Sync (On/Off)", "Sync Note L", "Sync Note R" };

static const char * const s_params_46[] = { "Mix", "Feedback", "High Pass", "Low Pass", "Drive", "Delay Time", "Wow", "Flutter", "Ping Pong (On/Off)", "Sync (On/Off)", "Sync Note" };

static const char * const s_params_47[] = { "Mix", "Size", "Pre Delay", "High Pass", "Low Pass" };

static const char * const s_params_48[] = { "Mix", "Decay", "Pre Delay", "Damping", "High Pass", "Low Pass" };

static const char * const s_params_49[] = { "Mix", "Decay", "Pre Delay", "High Pass", "Low Pass" };

static const char * const s_params_50[] = { "Mix", "Decay", "Pre Delay", "High Pass", "Low Pass", "Damping" };

static const char * const s_params_51[] = { "Mix", "Decay", "Pre Delay", "Mod Speed", "Mod Depth", "High Pass", "Low Pass" };

static const char * const s_params_52[] = { "Mix", "Decay", "Pre Delay", "High Pass", "Low Pass" };


const nano_fx_model_def_t nano_fx_catalog[] = {

    { "Chief BD2", 1, { 0x12 }, 3, s_params_0 },
    { "Chief OD1", 1, { 0x0D }, 2, s_params_1 },
    { "Exotic", 1, { 0x06 }, 4, s_params_2 },
    { "Exotic Bass Z Boost", 2, { 0xBF, 0x17 }, 4, s_params_3 },
    { "Exotic Z Boost", 1, { 0x17 }, 4, s_params_4 },
    { "Facial Fuzz", 1, { 0x16 }, 4, s_params_5 },
    { "Green 808", 1, { 0x1B }, 3, s_params_6 },
    { "Microtubes B3K", 2, { 0xB8, 0x17 }, 6, s_params_7 },
    { "OD250", 1, { 0x03 }, 2, s_params_8 },
    { "Obsessive Drive", 1, { 0x02 }, 4, s_params_9 },
    { "Rodent Drive", 1, { 0x04 }, 3, s_params_10 },
    { "Adaptive Gate", 2, { 0x81, 0x7D }, 1, s_params_11 },
    { "Graphic 9", 2, { 0xA5, 0x1F }, 12, s_params_12 },
    { "Low-High Cut", 2, { 0xA3, 0x1F }, 5, s_params_13 },
    { "Parametric 3", 2, { 0xA1, 0x1F }, 16, s_params_14 },
    { "Utility Gate", 2, { 0x82, 0x7D }, 5, s_params_15 },
    { "Volume", 2, { 0x86, 0x7D }, 2, s_params_16 },
    { "Bass Wah", 2, { 0xB4, 0x46 }, 1, s_params_17 },
    { "Bubba Wah", 2, { 0xB2, 0x46 }, 1, s_params_18 },
    { "Crying Clyde Wah", 2, { 0xB6, 0x46 }, 1, s_params_19 },
    { "Crying Wah", 2, { 0xB5, 0x46 }, 1, s_params_20 },
    { "Envelope Filter", 3, { 0xC6, 0xBB, 0x01 }, 9, s_params_21 },
    { "Love Meat", 3, { 0xC1, 0xBB, 0x01 }, 11, s_params_22 },
    { "Legendary 87 (M)", 2, { 0x89, 0x27 }, 6, s_params_23 },
    { "Opto Comp (M)", 2, { 0x8F, 0x27 }, 6, s_params_24 },
    { "Solid State Comp (M)", 2, { 0x8C, 0x27 }, 6, s_params_25 },
    { "VCA Comp (M)", 2, { 0x8D, 0x27 }, 6, s_params_26 },
    { "Transpose", 3, { 0xD1, 0x8C, 0x01 }, 5, s_params_27 },
    { "Doubler", 2, { 0x8B, 0x7D }, 3, s_params_28 },
    { "Legendary 87 (ST)", 2, { 0x94, 0x27 }, 6, s_params_29 },
    { "Opto Comp (ST)", 2, { 0x97, 0x27 }, 6, s_params_30 },
    { "Solid State Comp (ST)", 2, { 0x95, 0x27 }, 6, s_params_31 },
    { "VCA Comp (ST)", 2, { 0x96, 0x27 }, 6, s_params_32 },
    { "Chief CE2W (ST)", 2, { 0xF0, 0x36 }, 8, s_params_33 },
    { "Chief DC2W (ST)", 2, { 0xF3, 0x36 }, 5, s_params_34 },
    { "Chorus 229T", 2, { 0xEF, 0x36 }, 7, s_params_35 },
    { "Dream Chorus", 2, { 0xEE, 0x36 }, 7, s_params_36 },
    { "MX Flanger", 2, { 0xED, 0x36 }, 8, s_params_37 },
    { "MX Phase 95", 2, { 0xF4, 0x36 }, 7, s_params_38 },
    { "MX Vibe", 2, { 0xF5, 0x36 }, 8, s_params_39 },
    { "Tremolo", 2, { 0xDC, 0x36 }, 12, s_params_40 },
    { "Analog Delay", 2, { 0xFA, 0x2E }, 12, s_params_41 },
    { "Circular Delay", 2, { 0xFF, 0x2E }, 12, s_params_42 },
    { "Digital Delay (ST)", 2, { 0xFB, 0x2E }, 18, s_params_43 },
    { "Dual Delay", 2, { 0xFC, 0x2E }, 22, s_params_44 },
    { "Dual Reverse Delay", 2, { 0xFE, 0x2E }, 22, s_params_45 },
    { "Tape Delay", 2, { 0xF4, 0x2E }, 11, s_params_46 },
    { "Ambience", 2, { 0xC8, 0x3E }, 5, s_params_47 },
    { "Cave", 2, { 0xC9, 0x3E }, 6, s_params_48 },
    { "Hall", 2, { 0xC3, 0x3E }, 5, s_params_49 },
    { "Mind Hall", 2, { 0xCB, 0x3E }, 6, s_params_50 },
    { "Modulated", 2, { 0xC7, 0x3E }, 7, s_params_51 },
    { "Room", 2, { 0xC0, 0x3E }, 5, s_params_52 },

};

const size_t nano_fx_catalog_count = 53;
