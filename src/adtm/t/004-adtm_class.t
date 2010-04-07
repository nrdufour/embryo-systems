#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
    etap:plan(unknown),
    etap_can:loaded_ok(adtm_class, "Module 'adtm_class' loaded."),
    etap_can:has_attrib(adtm_class, behavior),
    etap_can:is_attrib(adtm_class, behavior, gen_server),
    etap_can:can_ok(adtm_class, create, 1),
    etap_can:can_ok(adtm_class, hibern, 1),
    etap_can:can_ok(adtm_class, awake, 1),
    etap_can:can_ok(adtm_class, destroy, 1),
    etap_can:can_ok(adtm_class, resur, 1),
    etap_can:can_ok(adtm_class, purge, 1),

    application:start(adtm),
    adtm_storage_server:init_storage(),
    etap:is(adtm_class:create("Bridge"), {ok, alive}, "Creating Class Bridge"),
    etap:is(adtm_class:create("Bridge"), {error, already_exists}, "Try to create Class Bridge again"),
    etap:is(adtm_class:create("River"), {ok, alive}, "Creating Class River"),
    etap:is(adtm_class:create("River"), {error, already_exists}, "Try to create Class River again"),

    etap:is(adtm_class:hibern("River"), {ok, frozen}, "Hibernate Class River"),
    etap:is(adtm_class:hibern("River"), {error, wrong_state}, "Try to hibernate again Class River"),
    etap:is(adtm_class:hibern("Car"), {error, not_found}, "Try to hibernate a class which doesn't exist"),
    application:stop(adtm),

    etap:end_tests(),
    ok.

