-- Model catalogs have no product-level count quota; individual model names remain validated.
alter table public.scribe_ai_model_profiles
  drop constraint if exists scribe_ai_model_profiles_models_check;
